//! The companion duties of stage 2f inside the session: notifications, find my
//! phone / find my watch and music control.
//!
//! Each duty is a setting of the host (`ClientConfig::duties`). With a duty off
//! the client behaves exactly as before 2f: the watch's messages for it get the
//! "not supported" answer (status 2) and nothing is sent unasked. With a duty on:
//!
//! - **Notifications.** The watch subscribes (5036); the host then posts and
//!   removes notifications (`Input::PostNotification`, `Input::RemoveNotification`).
//!   Each becomes a 5033 update. The watch asks for the attributes it wants on
//!   5034; the answer is built from what the host gave and sent as 5035 chunks,
//!   one at a time, each waiting for the watch's "OK". What the wearer does on the
//!   watch comes back as `Event::NotificationAction`.
//! - **Find my phone.** 5039 / 5040 become `Event::FindPhoneStart` /
//!   `Event::FindPhoneStop`. Separately, `Input::FindWatch` asks the watch to ring
//!   (protobuf `find_my_watch`), when the watch says it can.
//! - **Music.** 5042 is answered with the commands we handle, 5041 becomes
//!   `Event::MusicCommand`, and the host pushes the now-playing track, state and
//!   volume (`Input::MusicInfo`, `Input::MusicState`, `Input::MusicVolume`) as 5049.
//!
//! The behaviour is ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`:
//! `NotificationsHandler.java`, `GarminSupport.java` (`evaluateGBDeviceEvent`
//! for the subscription, `onNotification`, `onDeleteNotification`, `onFindDevice`,
//! `onSetMusicInfo`, `onSetMusicState`, `onSetPhoneVolume`; Copyright (C)
//! 2024-2026 Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne and the other
//! Gadgetbridge contributors) and the `messages/` classes named in
//! `crate::duties`. It is *inferred (Gadgetbridge)* until a watch confirms it.
//! What we do differently is marked [ours]: messages to the watch are soft
//! (a refusal or silence is an `Anomaly`, never the end of the session),
//! now-playing updates replace one another while they wait, and at most eight
//! attribute answers wait for the wire.
//!
//! Privacy: nothing here is logged or put into an event but what the host gave
//! for that purpose; `Debug` of the notification types hides their content.

use std::collections::VecDeque;

use super::session::{response, Kind, Out, Requests};
use super::{AnomalyKind, Duties, Event, FindWatchState, Now};
use crate::duties::find::{self, FindWatchMessage};
use crate::duties::music;
use crate::duties::notification::{
    self as notif, Control, ControlError, NotificationCategory, PhoneNotification, Transfer,
    UpdateType,
};
use crate::gfdi::Message;
use crate::messages::PROTOBUF_REQUEST;

/// "Unknown or not supported" status (see `session.rs`).
const STATUS_UNSUPPORTED: u8 = 2;

/// Attribute answers waiting for the wire (one is in flight). Further ones are
/// dropped; the watch asks again or shows less. [ours]
const MAX_TRANSFERS: usize = 8;

/// What the music duty pushes: the track.
#[derive(Clone, PartialEq, Eq)]
pub struct MusicInfo {
    pub artist: String,
    pub album: String,
    pub title: String,
    /// Track length in seconds, 0 when unknown.
    pub duration_s: u32,
}

impl std::fmt::Debug for MusicInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicInfo")
            .field("duration_s", &self.duration_s)
            .finish_non_exhaustive()
    }
}

/// What the music duty pushes: the player state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicState {
    pub playing: bool,
    /// Playback speed in percent (100 = normal); 0 is read as normal.
    pub rate_percent: u16,
    /// Position in whole seconds.
    pub position_s: u32,
}

/// The slice of the session the duties need for one call.
pub struct Cx<'a> {
    pub now: &'a Now,
    pub req: &'a mut Requests,
    pub out: &'a mut Out,
}

#[derive(Debug)]
pub struct Companion {
    duties: Duties,
    /// The watch has subscribed to notifications (5036 with enable).
    subscribed: bool,
    /// Notifications the watch knows about, oldest first.
    known: VecDeque<PhoneNotification>,
    /// Attribute answers: the front one is being sent.
    transfers: VecDeque<Transfer>,
    /// A 5035 chunk is queued or unanswered.
    chunk_in_flight: bool,
}

impl Companion {
    pub fn new(duties: Duties) -> Self {
        Self {
            duties,
            subscribed: false,
            known: VecDeque::new(),
            transfers: VecDeque::new(),
            chunk_in_flight: false,
        }
    }

    // ---- notifications ------------------------------------------------------

    /// 5036 from the watch (only called with the duty on).
    pub fn on_subscription(&mut self, msg: &Message, cx: &mut Cx) {
        let Some(enable) = notif::parse_subscription(&msg.payload) else {
            cx.out.anomaly(AnomalyKind::BadFrame, msg.id);
            return;
        };
        self.subscribed = enable;
        if !enable {
            self.known.clear();
        }
        cx.out.msgs.push(response(
            msg,
            0,
            &notif::subscription_reply_body(true, enable),
        ));
        cx.out
            .events
            .push(Event::NotificationSubscription { enabled: enable });
    }

    /// 5034 from the watch (only called with the duty on).
    pub fn on_control(&mut self, msg: &Message, cx: &mut Cx) {
        let control = match notif::parse_control(&msg.payload) {
            Ok(c) => c,
            Err(ControlError::Malformed) => {
                cx.out.anomaly(AnomalyKind::BadFrame, msg.id);
                return;
            }
            Err(ControlError::Unknown) => {
                cx.out.msgs.push(response(msg, STATUS_UNSUPPORTED, &[]));
                cx.out
                    .events
                    .push(Event::Unsupported { message_id: msg.id });
                return;
            }
        };
        cx.out.msgs.push(response(msg, 0, &notif::CONTROL_ACK_BODY));
        if !self.subscribed {
            return;
        }
        match control {
            Control::GetNotificationAttributes { id, attributes } => {
                if let Some(n) = self.known.iter().find(|n| n.id == id) {
                    let data = notif::build_notification_data(
                        n,
                        &attributes,
                        cx.now.utc_offset_s,
                        cx.now.unix_s,
                    );
                    self.start_transfer(data, cx);
                }
            }
            Control::GetAppAttributes { app_id, attributes } => {
                let name = self
                    .known
                    .iter()
                    .find(|n| n.app_id == app_id)
                    .map(|n| n.app_name.as_str());
                let data = notif::build_app_data(&app_id, name, &attributes);
                self.start_transfer(data, cx);
            }
            Control::LegacyAction { .. } => {}
            Control::Action { id, action } => {
                if self.known.iter().any(|n| n.id == id) {
                    cx.out.events.push(Event::NotificationAction { id, action });
                }
            }
        }
    }

    /// `Input::PostNotification`: announce a new or changed notification.
    pub fn post(&mut self, n: PhoneNotification, cx: &mut Cx) {
        if !self.duties.notifications || !self.subscribed {
            return;
        }
        let mut n = n.clamped();
        if n.when_unix_s == 0 {
            n.when_unix_s = cx.now.unix_s;
        }
        let id = n.id;
        let category = n.category;
        let existed = match self.known.iter().position(|k| k.id == id) {
            Some(i) => {
                self.known.remove(i);
                true
            }
            None => false,
        };
        self.known.push_back(n);
        self.send_update(update_kind(existed), category, id, true, cx);
        if self.known.len() > notif::MAX_NOTIFICATIONS {
            if let Some(old) = self.known.pop_front() {
                self.send_update(UpdateType::Remove, old.category, old.id, false, cx);
            }
        }
    }

    /// `Input::RemoveNotification`: the notification is gone on the phone.
    /// Sent even for an id we do not know (the watch may still show it from an
    /// earlier connection), as Gadgetbridge does.
    pub fn remove(&mut self, id: u32, cx: &mut Cx) {
        if !self.duties.notifications || !self.subscribed {
            return;
        }
        let category = match self.known.iter().position(|k| k.id == id) {
            Some(i) => self
                .known
                .remove(i)
                .map_or(NotificationCategory::Other, |n| n.category),
            None => NotificationCategory::Other,
        };
        self.send_update(UpdateType::Remove, category, id, false, cx);
    }

    fn send_update(
        &mut self,
        kind: UpdateType,
        category: NotificationCategory,
        id: u32,
        has_actions: bool,
        cx: &mut Cx,
    ) {
        let count = self.known.iter().filter(|n| n.category == category).count();
        let payload = notif::build_update(
            kind,
            category,
            count.min(usize::from(u8::MAX)) as u8,
            id,
            has_actions,
        );
        cx.req.send(
            cx.out,
            notif::NOTIFICATION_UPDATE,
            payload,
            Kind::NotificationUpdate,
        );
    }

    fn start_transfer(&mut self, data: Vec<u8>, cx: &mut Cx) {
        if self.transfers.len() >= MAX_TRANSFERS {
            return;
        }
        if let Some(t) = Transfer::new(data) {
            self.transfers.push_back(t);
            self.pump_transfer(cx);
        }
    }

    /// Send the next chunk of the front transfer unless one is on its way.
    fn pump_transfer(&mut self, cx: &mut Cx) {
        while !self.chunk_in_flight {
            let Some(t) = self.transfers.front_mut() else {
                return;
            };
            match t.next_chunk() {
                Some(payload) => {
                    self.chunk_in_flight = true;
                    cx.req.send(
                        cx.out,
                        notif::NOTIFICATION_DATA,
                        payload,
                        Kind::NotificationData,
                    );
                }
                None => {
                    self.transfers.pop_front();
                }
            }
        }
    }

    /// The watch answered a 5035 chunk. `ok` is "ACK with transfer status OK".
    pub fn on_data_answer(&mut self, ok: bool, cx: &mut Cx) {
        self.chunk_in_flight = false;
        if !ok {
            // Gadgetbridge gives the transfer up when the watch cannot go on.
            self.transfers.pop_front();
            cx.out
                .anomaly(AnomalyKind::DutyMessageRefused, notif::NOTIFICATION_DATA);
        } else if self.transfers.front().is_some_and(Transfer::done) {
            self.transfers.pop_front();
        }
        self.pump_transfer(cx);
    }

    /// A 5035 chunk got no answer in time: give that transfer up.
    pub fn drop_transfer(&mut self, cx: &mut Cx) {
        self.chunk_in_flight = false;
        self.transfers.pop_front();
        self.pump_transfer(cx);
    }

    // ---- find my phone / find my watch -------------------------------------------

    /// 5039 from the watch (only called with the duty on).
    pub fn on_find_phone_request(&mut self, msg: &Message, cx: &mut Cx) {
        match find::parse_find_phone_request(&msg.payload) {
            Some(duration_s) => {
                cx.out.msgs.push(response(msg, 0, &[]));
                cx.out.events.push(Event::FindPhoneStart { duration_s });
            }
            None => cx.out.anomaly(AnomalyKind::BadFrame, msg.id),
        }
    }

    /// 5040 from the watch (only called with the duty on).
    pub fn on_find_phone_cancel(&mut self, msg: &Message, cx: &mut Cx) {
        cx.out.msgs.push(response(msg, 0, &[]));
        cx.out.events.push(Event::FindPhoneStop);
    }

    /// `Input::FindWatch`: ask the watch to ring (`start`) or stop. Needs the
    /// watch's FIND_MY_WATCH capability; without it only an event is emitted.
    pub fn find_watch(&mut self, start: bool, watch_capabilities: &[u8], cx: &mut Cx) {
        if !find::has_capability(watch_capabilities, find::CAP_FIND_MY_WATCH) {
            cx.out.events.push(Event::FindWatch {
                state: FindWatchState::NotSupported,
            });
            return;
        }
        let body: &[u8] = if start {
            &find::FIND_WATCH_START_PROTOBUF
        } else {
            &find::FIND_WATCH_CANCEL_PROTOBUF
        };
        let id = cx.req.next_protobuf_id();
        cx.req.send(
            cx.out,
            PROTOBUF_REQUEST,
            crate::messages::build_protobuf(id, body),
            Kind::FindWatch,
        );
    }

    /// The Smart bytes of a protobuf message from the watch: report what the
    /// `find_my_watch` service says, if anything.
    pub fn on_find_watch_protobuf(&mut self, body: &[u8], out: &mut Out) {
        let state = match find::parse_find_my_watch(body) {
            Some(FindWatchMessage::FindResponse { ok: true }) => FindWatchState::Ringing,
            Some(FindWatchMessage::CancelResponse { ok: true }) => FindWatchState::Stopped,
            Some(
                FindWatchMessage::FindResponse { ok: false }
                | FindWatchMessage::CancelResponse { ok: false },
            ) => FindWatchState::Failed,
            Some(FindWatchMessage::CancelRequest) => FindWatchState::FoundOnWatch,
            None => return,
        };
        out.events.push(Event::FindWatch { state });
    }

    // ---- music -----------------------------------------------------------------

    /// 5042 from the watch (only called with the duty on).
    pub fn on_music_capabilities(&mut self, msg: &Message, cx: &mut Cx) {
        cx.out
            .msgs
            .push(response(msg, 0, &music::capabilities_reply_body()));
        cx.out.events.push(Event::MusicRequested);
    }

    /// 5041 from the watch (only called with the duty on).
    pub fn on_music_control(&mut self, msg: &Message, cx: &mut Cx) {
        match music::parse_music_control(&msg.payload) {
            Some(command) => {
                cx.out.msgs.push(response(msg, 0, &[]));
                cx.out.events.push(Event::MusicCommand { command });
            }
            None if msg.payload.is_empty() => cx.out.anomaly(AnomalyKind::BadFrame, msg.id),
            None => {
                cx.out.msgs.push(response(msg, STATUS_UNSUPPORTED, &[]));
                cx.out
                    .events
                    .push(Event::Unsupported { message_id: msg.id });
            }
        }
    }

    /// 5049 from the watch (only called with the duty on): which attributes it
    /// wants. Acknowledged; we push what we have on our own schedule. [ours]
    pub fn on_music_entity_request(&mut self, msg: &Message, cx: &mut Cx) {
        cx.out.msgs.push(response(msg, 0, &[]));
    }

    /// `Input::MusicInfo`.
    pub fn music_info(&mut self, info: &MusicInfo, cx: &mut Cx) {
        if !self.duties.music {
            return;
        }
        let payload = music::build_entity_update(&[
            (music::TRACK_ARTIST, info.artist.clone()),
            (music::TRACK_ALBUM, info.album.clone()),
            (music::TRACK_TITLE, info.title.clone()),
            (music::TRACK_DURATION, info.duration_s.to_string()),
        ]);
        cx.req
            .send_replacing(cx.out, music::MUSIC_ENTITY_UPDATE, payload, Kind::MusicInfo);
    }

    /// `Input::MusicState`.
    pub fn music_state(&mut self, state: MusicState, cx: &mut Cx) {
        if !self.duties.music {
            return;
        }
        let value = music::playback_info(state.playing, state.rate_percent, state.position_s);
        let payload = music::build_entity_update(&[(music::PLAYER_PLAYBACK_INFO, value)]);
        cx.req.send_replacing(
            cx.out,
            music::MUSIC_ENTITY_UPDATE,
            payload,
            Kind::MusicState,
        );
    }

    /// `Input::MusicVolume`.
    pub fn music_volume(&mut self, percent: u8, cx: &mut Cx) {
        if !self.duties.music {
            return;
        }
        let payload =
            music::build_entity_update(&[(music::PLAYER_VOLUME, music::volume_value(percent))]);
        cx.req.send_replacing(
            cx.out,
            music::MUSIC_ENTITY_UPDATE,
            payload,
            Kind::MusicVolume,
        );
    }
}

/// An update for an id the watch already knows is a MODIFY, a new one an ADD.
fn update_kind(existed: bool) -> UpdateType {
    if existed {
        UpdateType::Modify
    } else {
        UpdateType::Add
    }
}
