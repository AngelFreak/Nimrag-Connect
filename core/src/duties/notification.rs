//! Notifications on the watch: 5033 (update, phone to watch), 5034 (control
//! point, watch to phone), 5035 (data, phone to watch, in chunks) and 5036
//! (subscription, watch to phone). An ANCS-like attribute protocol: the phone
//! announces a notification, the watch asks for the attributes it wants, the
//! phone answers in chunks, and the watch reports actions (dismiss, reply, ...).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`:
//! `NotificationsHandler.java` (command, attribute and action tables, the
//! attribute encoders, the chunked upload), `messages/NotificationUpdateMessage.java`,
//! `messages/NotificationControlMessage.java`, `messages/NotificationDataMessage.java`,
//! `messages/NotificationSubscriptionMessage.java` and
//! `messages/status/Notification{Control,Data,Subscription}StatusMessage.java`.
//! None of these files has a copyright line; Gadgetbridge contributors.
//! *inferred (Gadgetbridge)* until a watch confirms it.
//!
//! Differences from Gadgetbridge are marked [ours]: attribute text is cut to the
//! watch's maximum in bytes at a character boundary (Gadgetbridge counts UTF-16
//! units), the date is formatted from the phone's UTC offset instead of the
//! JVM's default zone, an attachment count of 0 is answered (Gadgetbridge
//! answers 1, but we never announce attachments), and the only action we offer is
//! "Dismiss".
//!
//! Privacy: `PhoneNotification` and `NotificationAction::Reply` carry
//! notification content. Their `Debug` output hides it; never log the fields.

use std::fmt;

use super::truncate_utf8;
use crate::crc;

pub const NOTIFICATION_UPDATE: u16 = 5033;
pub const NOTIFICATION_CONTROL: u16 = 5034;
pub const NOTIFICATION_DATA: u16 = 5035;
pub const NOTIFICATION_SUBSCRIPTION: u16 = 5036;

/// Capability bit for notifications (`GarminCapability.GNCS`).
pub const CAP_GNCS: usize = 6;

/// Most notifications the phone side remembers at once (Gadgetbridge: 64).
pub const MAX_NOTIFICATIONS: usize = 64;

/// Bytes of attribute data per 5035 chunk (Gadgetbridge `maxBlockSize`).
pub const DATA_CHUNK: usize = 300;

/// Limits on what we keep of a notification, in bytes. [ours]
pub const MAX_APP_ID: usize = 255;
pub const MAX_APP_NAME: usize = 255;
pub const MAX_TITLE: usize = 512;
pub const MAX_SUBTITLE: usize = 512;
pub const MAX_BODY: usize = 4096;

/// The one action we offer on a notification (code 98, `DISMISS_NOTIFICATION`).
/// The label is shown on the watch. [ours]
pub const DISMISS_LABEL: &str = "Dismiss";

/// What kind of notification this is, as far as the watch cares (the icon and
/// the category number of `NotificationUpdateMessage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCategory {
    Other,
    Email,
    /// Messages and chat (Gadgetbridge sends both as SMS).
    Message,
    Social,
    /// Navigation.
    Location,
}

impl NotificationCategory {
    fn wire(self) -> u8 {
        match self {
            NotificationCategory::Other => 0,
            NotificationCategory::Social => 4,
            NotificationCategory::Email => 6,
            NotificationCategory::Location => 10,
            NotificationCategory::Message => 12,
        }
    }
}

/// A notification the phone shows, as far as the watch needs it.
#[derive(Clone, PartialEq, Eq)]
pub struct PhoneNotification {
    /// Chosen by the host; the watch names the notification by it. Reuse an id
    /// to update the notification.
    pub id: u32,
    /// The posting app's identifier (the Android package name).
    pub app_id: String,
    /// The app's display name, answered when the watch asks for it.
    pub app_name: String,
    pub category: NotificationCategory,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    /// When it was posted, Unix seconds. 0 means "when it reached us".
    pub when_unix_s: i64,
}

impl fmt::Debug for PhoneNotification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Content is never printed.
        f.debug_struct("PhoneNotification")
            .field("id", &self.id)
            .field("category", &self.category)
            .finish_non_exhaustive()
    }
}

impl PhoneNotification {
    /// The notification with every text cut to the limits above.
    pub fn clamped(mut self) -> Self {
        fn cut(s: &mut String, max: usize) {
            let n = truncate_utf8(s, max).len();
            s.truncate(n);
        }
        cut(&mut self.app_id, MAX_APP_ID);
        cut(&mut self.app_name, MAX_APP_NAME);
        cut(&mut self.title, MAX_TITLE);
        cut(&mut self.subtitle, MAX_SUBTITLE);
        cut(&mut self.body, MAX_BODY);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateType {
    Add,
    Modify,
    Remove,
}

impl UpdateType {
    fn wire(self) -> u8 {
        match self {
            UpdateType::Add => 0,
            UpdateType::Modify => 1,
            UpdateType::Remove => 2,
        }
    }
}

/// Category flags of every update: FOREGROUND (bit 1) and ACTION_DECLINE
/// (bit 4), as Gadgetbridge sends for every type with new-style actions.
const CATEGORY_FLAGS: u8 = 0x12;
/// Phone flag NEW_ACTIONS (bit 1): the notification has actions.
const PHONE_FLAG_NEW_ACTIONS: u8 = 0x02;

/// 5033 payload: `u8 update type`, `u8 category flags`, `u8 category`, `u8 count`
/// (notifications of this category now known), `u32 id`, `u8 phone flags`.
pub fn build_update(
    kind: UpdateType,
    category: NotificationCategory,
    count: u8,
    id: u32,
    has_actions: bool,
) -> Vec<u8> {
    let mut v = vec![kind.wire(), CATEGORY_FLAGS, category.wire(), count];
    v.extend_from_slice(&id.to_le_bytes());
    v.push(if has_actions {
        PHONE_FLAG_NEW_ACTIONS
    } else {
        0
    });
    v
}

/// 5036 payload: `u8 enable` (1 = subscribe), then an unknown byte we ignore.
/// `None` for an empty payload.
pub fn parse_subscription(p: &[u8]) -> Option<bool> {
    Some(*p.first()? == 1)
}

/// Body of the answer to 5036 (after the status byte): `u8` 0 enabled / 1
/// disabled on our side, `u8` the watch's enable value echoed, `u8` 0.
pub fn subscription_reply_body(accepted: bool, enable: bool) -> Vec<u8> {
    vec![u8::from(!accepted), u8::from(enable), 0]
}

/// Body of the answer to every understood 5034 (after the status byte): chunk
/// status OK, status code NO_ERROR.
pub const CONTROL_ACK_BODY: [u8; 2] = [0, 0];

/// A notification attribute the watch can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    AppIdentifier,
    Title,
    Subtitle,
    Message,
    MessageSize,
    Date,
    NegativeActionLabel,
    Actions,
    Attachments,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Params {
    None,
    /// `u16` maximum length.
    Length,
    /// `u16` maximum length and one more byte (Gadgetbridge: "what is this?").
    LengthAndByte,
}

impl Attribute {
    fn code(self) -> u8 {
        match self {
            Attribute::AppIdentifier => 0,
            Attribute::Title => 1,
            Attribute::Subtitle => 2,
            Attribute::Message => 3,
            Attribute::MessageSize => 4,
            Attribute::Date => 5,
            Attribute::NegativeActionLabel => 7,
            Attribute::Actions => 127,
            Attribute::Attachments => 128,
        }
    }

    fn from_code(code: u8) -> Option<Attribute> {
        Some(match code {
            0 => Attribute::AppIdentifier,
            1 => Attribute::Title,
            2 => Attribute::Subtitle,
            3 => Attribute::Message,
            4 => Attribute::MessageSize,
            5 => Attribute::Date,
            7 => Attribute::NegativeActionLabel,
            127 => Attribute::Actions,
            128 => Attribute::Attachments,
            _ => return None,
        })
    }

    fn params(self) -> Params {
        match self {
            Attribute::Title | Attribute::Subtitle | Attribute::Message => Params::Length,
            Attribute::Actions => Params::LengthAndByte,
            _ => Params::None,
        }
    }
}

/// One attribute the watch asked for, with the maximum length it gave
/// (0 = none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeRequest {
    pub attribute: Attribute,
    pub max_len: u16,
}

/// The app attributes the watch can ask for (only the name is known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAttribute {
    AppName,
}

/// What the wearer did on the watch.
#[derive(Clone, PartialEq, Eq)]
pub enum NotificationAction {
    /// Dismiss the notification (code 98).
    Dismiss,
    /// Block the app (code 99, "mute" in Gadgetbridge).
    Block,
    /// A canned or typed reply (codes 95 and 94). The text is private.
    Reply {
        text: String,
    },
    AcceptCall,
    RejectCall,
    /// The n-th simple action of the notification, counted from 0 (codes 1-5).
    Custom {
        index: u8,
    },
}

impl fmt::Debug for NotificationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationAction::Dismiss => f.write_str("Dismiss"),
            NotificationAction::Block => f.write_str("Block"),
            NotificationAction::Reply { .. } => f.write_str("Reply { .. }"),
            NotificationAction::AcceptCall => f.write_str("AcceptCall"),
            NotificationAction::RejectCall => f.write_str("RejectCall"),
            NotificationAction::Custom { index } => write!(f, "Custom {{ index: {index} }}"),
        }
    }
}

fn action_from_code(code: u8, text: Option<String>) -> Option<NotificationAction> {
    Some(match code {
        1..=5 => NotificationAction::Custom { index: code - 1 },
        94 | 95 => NotificationAction::Reply {
            text: text.unwrap_or_default(),
        },
        96 => NotificationAction::AcceptCall,
        97 => NotificationAction::RejectCall,
        98 => NotificationAction::Dismiss,
        99 => NotificationAction::Block,
        _ => return None,
    })
}

/// A request from the watch on the control point (5034).
#[derive(Clone, PartialEq, Eq)]
pub enum Control {
    /// Command 0: send these attributes of this notification.
    GetNotificationAttributes {
        id: u32,
        attributes: Vec<AttributeRequest>,
    },
    /// Command 1: send these attributes of this app.
    GetAppAttributes {
        app_id: String,
        attributes: Vec<AppAttribute>,
    },
    /// Command 2: the legacy accept / refuse of ANCS. Acknowledged, not acted on.
    LegacyAction { id: u32, accept: bool },
    /// Command 128: an action on a notification.
    Action { id: u32, action: NotificationAction },
}

impl fmt::Debug for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Control::GetNotificationAttributes { id, attributes } => f
                .debug_struct("GetNotificationAttributes")
                .field("id", id)
                .field("attributes", attributes)
                .finish(),
            // The app id is not printed: which app posted is private too.
            Control::GetAppAttributes { attributes, .. } => f
                .debug_struct("GetAppAttributes")
                .field("attributes", attributes)
                .finish_non_exhaustive(),
            Control::LegacyAction { id, accept } => f
                .debug_struct("LegacyAction")
                .field("id", id)
                .field("accept", accept)
                .finish(),
            Control::Action { id, action } => f
                .debug_struct("Action")
                .field("id", id)
                .field("action", action)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlError {
    /// Truncated or otherwise not parseable.
    Malformed,
    /// A command, attribute or action number we do not know. Gadgetbridge
    /// treats these as unhandled messages (status 2).
    Unknown,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.b.len() - self.i
    }

    fn u8(&mut self) -> Result<u8, ControlError> {
        let v = *self.b.get(self.i).ok_or(ControlError::Malformed)?;
        self.i += 1;
        Ok(v)
    }

    fn u16(&mut self) -> Result<u16, ControlError> {
        Ok(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    fn u32(&mut self) -> Result<u32, ControlError> {
        Ok(u32::from_le_bytes([
            self.u8()?,
            self.u8()?,
            self.u8()?,
            self.u8()?,
        ]))
    }

    /// Bytes up to a NUL (consumed) or the end.
    fn cstring(&mut self) -> String {
        let rest = &self.b[self.i..];
        let n = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.i += (n + 1).min(rest.len());
        s
    }
}

/// Decode a 5034 payload.
pub fn parse_control(p: &[u8]) -> Result<Control, ControlError> {
    let mut r = Reader { b: p, i: 0 };
    match r.u8()? {
        0 => {
            let id = r.u32()?;
            let mut attributes = Vec::new();
            while r.remaining() > 0 {
                let attribute = Attribute::from_code(r.u8()?).ok_or(ControlError::Unknown)?;
                let max_len = match attribute.params() {
                    Params::None => 0,
                    Params::Length => r.u16()?,
                    Params::LengthAndByte => {
                        let n = r.u16()?;
                        r.u8()?;
                        n
                    }
                };
                attributes.push(AttributeRequest { attribute, max_len });
            }
            Ok(Control::GetNotificationAttributes { id, attributes })
        }
        1 => {
            // The identifier ends with a NUL; without one the message is cut.
            if !p[1..].contains(&0) {
                return Err(ControlError::Malformed);
            }
            let app_id = r.cstring();
            let mut attributes = Vec::new();
            while r.remaining() > 0 {
                match r.u8()? {
                    0 => attributes.push(AppAttribute::AppName),
                    _ => return Err(ControlError::Unknown),
                }
            }
            Ok(Control::GetAppAttributes { app_id, attributes })
        }
        2 => {
            let id = r.u32()?;
            match r.u8()? {
                0 => Ok(Control::LegacyAction { id, accept: true }),
                1 => Ok(Control::LegacyAction { id, accept: false }),
                _ => Err(ControlError::Unknown),
            }
        }
        128 => {
            let id = r.u32()?;
            let code = r.u8()?;
            // Non-reply actions may carry no string at all on recent firmware.
            let text = (r.remaining() > 0).then(|| r.cstring());
            let action = action_from_code(code, text).ok_or(ControlError::Unknown)?;
            Ok(Control::Action { id, action })
        }
        _ => Err(ControlError::Unknown),
    }
}

/// `value` of the ACTIONS attribute: `u8 count`, then per action `u8 code`, `u8
/// icon position bit`, `u8 label length` and the label. We offer Dismiss only
/// (code 98, icon position LEFT = bit 2).
fn actions_value() -> Vec<u8> {
    let label = DISMISS_LABEL.as_bytes();
    let mut v = vec![1, 98, 1 << 2, label.len() as u8];
    v.extend_from_slice(label);
    v
}

/// `yyyyMMdd'T'HHmmss` of a Unix time in a zone given by its UTC offset
/// (Gadgetbridge's `NOTIFICATION_DATE_FORMAT`).
pub fn format_date(unix_s: i64, utc_offset_s: i32) -> String {
    let t = unix_s.saturating_add(i64::from(utc_offset_s));
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn push_attribute(out: &mut Vec<u8>, attribute: Attribute, value: &[u8]) {
    out.push(attribute.code());
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
}

/// The answer to command 0: `u8 0`, `u32 id`, then for each requested
/// attribute `u8 code`, `u16 length` and the value. The message size goes last
/// whatever its place in the request, and an attribute asked for twice is
/// answered once, at its first place with its last maximum.
pub fn build_notification_data(
    n: &PhoneNotification,
    requests: &[AttributeRequest],
    utc_offset_s: i32,
    now_unix_s: i64,
) -> Vec<u8> {
    let mut wanted: Vec<AttributeRequest> = Vec::new();
    for r in requests {
        match wanted.iter_mut().find(|w| w.attribute == r.attribute) {
            Some(w) => w.max_len = r.max_len,
            None => wanted.push(*r),
        }
    }
    let size_last = wanted
        .iter()
        .position(|w| w.attribute == Attribute::MessageSize)
        .map(|i| wanted.remove(i));
    wanted.extend(size_last);

    let mut out = vec![0u8];
    out.extend_from_slice(&n.id.to_le_bytes());
    for w in wanted {
        let max = usize::from(w.max_len);
        let text = |s: &str| truncate_utf8(s, max).as_bytes().to_vec();
        let value = match w.attribute {
            Attribute::AppIdentifier => n.app_id.as_bytes().to_vec(),
            Attribute::Title => text(&n.title),
            Attribute::Subtitle => text(&n.subtitle),
            Attribute::Message => text(&n.body),
            // The number of UTF-16 units, as Java's `String.length()`.
            Attribute::MessageSize => n.body.encode_utf16().count().to_string().into_bytes(),
            Attribute::Date => {
                let when = if n.when_unix_s == 0 {
                    now_unix_s
                } else {
                    n.when_unix_s
                };
                format_date(when, utc_offset_s).into_bytes()
            }
            Attribute::NegativeActionLabel => Vec::new(),
            Attribute::Actions => actions_value(),
            Attribute::Attachments => b"0".to_vec(),
        };
        push_attribute(&mut out, w.attribute, &value);
    }
    out
}

/// The answer to command 1: `u8 1`, the app identifier and a NUL, then for each
/// attribute `u8 code`, `u16 length` and the value. An unknown app is called by
/// its identifier.
pub fn build_app_data(
    app_id: &str,
    app_name: Option<&str>,
    attributes: &[AppAttribute],
) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend_from_slice(app_id.as_bytes());
    out.push(0);
    for a in attributes {
        match a {
            AppAttribute::AppName => {
                let name = app_name
                    .filter(|n| !n.trim().is_empty())
                    .or(Some(app_id).filter(|n| !n.trim().is_empty()))
                    .unwrap_or("Unknown");
                out.push(0);
                out.extend_from_slice(&(name.len() as u16).to_le_bytes());
                out.extend_from_slice(name.as_bytes());
            }
        }
    }
    out
}

/// Attribute data on its way to the watch, one 5035 chunk at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    data: Vec<u8>,
    offset: usize,
    crc: u16,
}

impl Transfer {
    /// `None` for an empty message or one the 16-bit size field cannot hold.
    pub fn new(data: Vec<u8>) -> Option<Transfer> {
        if data.is_empty() || data.len() > usize::from(u16::MAX) {
            return None;
        }
        Some(Transfer {
            data,
            offset: 0,
            crc: 0,
        })
    }

    /// Every byte has been handed out.
    pub fn done(&self) -> bool {
        self.offset >= self.data.len()
    }

    /// The next 5035 payload: `u16 total size`, `u16 running CRC` (over the
    /// data up to and including this chunk), `u16 offset` of this chunk, and
    /// up to [`DATA_CHUNK`] bytes. `None` when everything was handed out.
    pub fn next_chunk(&mut self) -> Option<Vec<u8>> {
        if self.done() {
            return None;
        }
        let end = (self.offset + DATA_CHUNK).min(self.data.len());
        let chunk = &self.data[self.offset..end];
        self.crc = crc::update(self.crc, chunk);
        let mut v = Vec::with_capacity(6 + chunk.len());
        v.extend_from_slice(&(self.data.len() as u16).to_le_bytes());
        v.extend_from_slice(&self.crc.to_le_bytes());
        v.extend_from_slice(&(self.offset as u16).to_le_bytes());
        v.extend_from_slice(chunk);
        self.offset = end;
        Some(v)
    }
}

/// `true` when the watch's answer to a 5035 chunk (the byte after the status)
/// says "go on" (`TransferStatus.OK`; 1 resend, 2 abort, 3 CRC mismatch, 4
/// offset mismatch).
pub fn transfer_ok(body: &[u8]) -> bool {
    body.first() == Some(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PhoneNotification {
        PhoneNotification {
            id: 0x0102_0304,
            app_id: "org.example.chat".into(),
            app_name: "Example Chat".into(),
            category: NotificationCategory::Message,
            title: "Ann".into(),
            subtitle: String::new(),
            body: "Hello there".into(),
            when_unix_s: 1_800_000_000,
        }
    }

    #[test]
    fn update_layout_is_byte_exact() {
        let b = build_update(
            UpdateType::Add,
            NotificationCategory::Message,
            3,
            0x0102_0304,
            true,
        );
        assert_eq!(b, vec![0, 0x12, 12, 3, 4, 3, 2, 1, 0x02]);
        let r = build_update(UpdateType::Remove, NotificationCategory::Other, 0, 7, false);
        assert_eq!(r, vec![2, 0x12, 0, 0, 7, 0, 0, 0, 0]);
        assert_eq!(
            build_update(UpdateType::Modify, NotificationCategory::Email, 1, 1, true)[2],
            6
        );
        assert_eq!(
            build_update(UpdateType::Add, NotificationCategory::Social, 1, 1, true)[2],
            4
        );
        assert_eq!(
            build_update(UpdateType::Add, NotificationCategory::Location, 1, 1, true)[2],
            10
        );
    }

    #[test]
    fn subscription_and_its_reply() {
        assert_eq!(parse_subscription(&[1, 0]), Some(true));
        assert_eq!(parse_subscription(&[0]), Some(false));
        assert_eq!(parse_subscription(&[]), None);
        assert_eq!(subscription_reply_body(true, true), vec![0, 1, 0]);
        assert_eq!(subscription_reply_body(true, false), vec![0, 0, 0]);
        assert_eq!(subscription_reply_body(false, true), vec![1, 1, 0]);
    }

    #[test]
    fn get_attributes_parses_lengths_and_extra_bytes() {
        // id 5; title (max 32), message (max 200), message size, date, actions (max 1000 + 1 byte)
        let p = [0, 5, 0, 0, 0, 1, 32, 0, 3, 200, 0, 4, 5, 127, 0xE8, 0x03, 9];
        assert_eq!(
            parse_control(&p),
            Ok(Control::GetNotificationAttributes {
                id: 5,
                attributes: vec![
                    AttributeRequest {
                        attribute: Attribute::Title,
                        max_len: 32
                    },
                    AttributeRequest {
                        attribute: Attribute::Message,
                        max_len: 200
                    },
                    AttributeRequest {
                        attribute: Attribute::MessageSize,
                        max_len: 0
                    },
                    AttributeRequest {
                        attribute: Attribute::Date,
                        max_len: 0
                    },
                    AttributeRequest {
                        attribute: Attribute::Actions,
                        max_len: 1000
                    },
                ],
            })
        );
    }

    #[test]
    fn bad_control_messages_are_told_apart() {
        assert_eq!(parse_control(&[]), Err(ControlError::Malformed));
        // truncated id
        assert_eq!(parse_control(&[0, 1, 2]), Err(ControlError::Malformed));
        // title without its length
        assert_eq!(
            parse_control(&[0, 1, 0, 0, 0, 1]),
            Err(ControlError::Malformed)
        );
        // unknown attribute, command and action
        assert_eq!(
            parse_control(&[0, 1, 0, 0, 0, 9]),
            Err(ControlError::Unknown)
        );
        assert_eq!(parse_control(&[77, 0, 0]), Err(ControlError::Unknown));
        assert_eq!(
            parse_control(&[128, 1, 0, 0, 0, 55]),
            Err(ControlError::Unknown)
        );
        // an app identifier without its terminator
        assert_eq!(
            parse_control(&[1, b'a', b'b']),
            Err(ControlError::Malformed)
        );
    }

    #[test]
    fn actions_are_decoded() {
        let dismiss = parse_control(&[128, 9, 0, 0, 0, 98]);
        assert_eq!(
            dismiss,
            Ok(Control::Action {
                id: 9,
                action: NotificationAction::Dismiss
            })
        );
        let mut reply = vec![128, 9, 0, 0, 0, 95];
        reply.extend_from_slice(b"On my way\0");
        assert_eq!(
            parse_control(&reply),
            Ok(Control::Action {
                id: 9,
                action: NotificationAction::Reply {
                    text: "On my way".into()
                }
            })
        );
        // a reply string without its NUL is still read
        let mut cut = vec![128, 9, 0, 0, 0, 95];
        cut.extend_from_slice(b"ok");
        assert_eq!(
            parse_control(&cut),
            Ok(Control::Action {
                id: 9,
                action: NotificationAction::Reply { text: "ok".into() }
            })
        );
        for (code, want) in [
            (96, NotificationAction::AcceptCall),
            (97, NotificationAction::RejectCall),
            (99, NotificationAction::Block),
            (1, NotificationAction::Custom { index: 0 }),
            (5, NotificationAction::Custom { index: 4 }),
        ] {
            assert_eq!(
                parse_control(&[128, 1, 0, 0, 0, code]),
                Ok(Control::Action {
                    id: 1,
                    action: want
                })
            );
        }
    }

    #[test]
    fn app_attributes_and_legacy_actions() {
        let mut p = vec![1];
        p.extend_from_slice(b"org.example.chat\0");
        p.push(0);
        assert_eq!(
            parse_control(&p),
            Ok(Control::GetAppAttributes {
                app_id: "org.example.chat".into(),
                attributes: vec![AppAttribute::AppName]
            })
        );
        // an unknown app attribute
        p.push(7);
        assert_eq!(parse_control(&p), Err(ControlError::Unknown));
        assert_eq!(
            parse_control(&[2, 4, 0, 0, 0, 0]),
            Ok(Control::LegacyAction {
                id: 4,
                accept: true
            })
        );
        assert_eq!(
            parse_control(&[2, 4, 0, 0, 0, 1]),
            Ok(Control::LegacyAction {
                id: 4,
                accept: false
            })
        );
        assert_eq!(
            parse_control(&[2, 4, 0, 0, 0, 2]),
            Err(ControlError::Unknown)
        );
    }

    #[test]
    fn debug_output_never_shows_content() {
        let n = sample();
        let shown = format!("{n:?}");
        assert!(!shown.contains("Ann") && !shown.contains("Hello") && !shown.contains("chat"));
        let a = NotificationAction::Reply {
            text: "secret".into(),
        };
        assert!(!format!("{a:?}").contains("secret"));
        let c = Control::Action {
            id: 1,
            action: a.clone(),
        };
        assert!(!format!("{c:?}").contains("secret"));
        let c = Control::GetAppAttributes {
            app_id: "org.example.chat".into(),
            attributes: vec![],
        };
        assert!(!format!("{c:?}").contains("example"));
    }

    #[test]
    fn dates_are_formatted_like_the_java_pattern() {
        // 2027-01-15 08:00:00 UTC is 1_800_000_000
        assert_eq!(format_date(1_800_000_000, 0), "20270115T080000");
        assert_eq!(format_date(1_800_000_000, 3600), "20270115T090000");
        // across midnight and a year
        assert_eq!(format_date(1_800_000_000, 16 * 3600), "20270116T000000");
        assert_eq!(format_date(0, 0), "19700101T000000");
        assert_eq!(format_date(951_782_400, 0), "20000229T000000");
        assert_eq!(format_date(-1, 0), "19691231T235959");
    }

    #[test]
    fn notification_data_is_byte_exact() {
        let n = sample();
        let req = [
            AttributeRequest {
                attribute: Attribute::AppIdentifier,
                max_len: 0,
            },
            AttributeRequest {
                attribute: Attribute::Title,
                max_len: 100,
            },
            AttributeRequest {
                attribute: Attribute::MessageSize,
                max_len: 0,
            },
            AttributeRequest {
                attribute: Attribute::Message,
                max_len: 5,
            },
            AttributeRequest {
                attribute: Attribute::Date,
                max_len: 0,
            },
        ];
        let data = build_notification_data(&n, &req, 3600, 0);
        let mut want = vec![0, 4, 3, 2, 1];
        want.extend_from_slice(&[0, 16, 0]);
        want.extend_from_slice(b"org.example.chat");
        want.extend_from_slice(&[1, 3, 0]);
        want.extend_from_slice(b"Ann");
        want.extend_from_slice(&[3, 5, 0]);
        want.extend_from_slice(b"Hello");
        want.extend_from_slice(&[5, 15, 0]);
        want.extend_from_slice(b"20270115T090000");
        // the size is last, although it was asked for third
        want.extend_from_slice(&[4, 2, 0]);
        want.extend_from_slice(b"11");
        assert_eq!(data, want);
    }

    #[test]
    fn data_uses_now_when_the_notification_has_no_time_and_answers_the_small_attributes() {
        let mut n = sample();
        n.when_unix_s = 0;
        let req = [
            AttributeRequest {
                attribute: Attribute::Date,
                max_len: 0,
            },
            AttributeRequest {
                attribute: Attribute::NegativeActionLabel,
                max_len: 0,
            },
            AttributeRequest {
                attribute: Attribute::Attachments,
                max_len: 0,
            },
            AttributeRequest {
                attribute: Attribute::Subtitle,
                max_len: 10,
            },
            // asked twice: answered once at the first place, with the last max
            AttributeRequest {
                attribute: Attribute::Date,
                max_len: 8,
            },
        ];
        let data = build_notification_data(&n, &req, 0, 1_800_000_000);
        let mut want = vec![0, 4, 3, 2, 1, 5, 15, 0];
        want.extend_from_slice(b"20270115T080000");
        want.extend_from_slice(&[7, 0, 0]);
        want.extend_from_slice(&[128, 1, 0, b'0']);
        want.extend_from_slice(&[2, 0, 0]);
        assert_eq!(data, want);
    }

    #[test]
    fn actions_attribute_offers_dismiss() {
        let n = sample();
        let req = [AttributeRequest {
            attribute: Attribute::Actions,
            max_len: 1000,
        }];
        let data = build_notification_data(&n, &req, 0, 0);
        let mut want = vec![0, 4, 3, 2, 1, 127, 11, 0];
        want.extend_from_slice(&[1, 98, 4, 7]);
        want.extend_from_slice(b"Dismiss");
        assert_eq!(data, want);
    }

    #[test]
    fn message_size_counts_utf16_units() {
        let mut n = sample();
        n.body = "a😀é".into(); // 1 + 2 + 1 units
        let req = [AttributeRequest {
            attribute: Attribute::MessageSize,
            max_len: 0,
        }];
        let data = build_notification_data(&n, &req, 0, 0);
        assert_eq!(&data[5..], &[4, 1, 0, b'4']);
    }

    #[test]
    fn text_is_cut_to_the_watchs_maximum_in_bytes() {
        let mut n = sample();
        n.title = "ééé".into(); // 6 bytes
        let req = [AttributeRequest {
            attribute: Attribute::Title,
            max_len: 5,
        }];
        let data = build_notification_data(&n, &req, 0, 0);
        assert_eq!(&data[5..], &[1, 4, 0, 0xC3, 0xA9, 0xC3, 0xA9]);
    }

    #[test]
    fn app_data_falls_back_to_the_identifier() {
        assert_eq!(
            build_app_data("org.x", Some("X App"), &[AppAttribute::AppName]),
            [&[1u8][..], b"org.x", &[0, 0, 5, 0], b"X App"].concat()
        );
        assert_eq!(
            build_app_data("org.x", None, &[AppAttribute::AppName]),
            [&[1u8][..], b"org.x", &[0, 0, 5, 0], b"org.x"].concat()
        );
        assert_eq!(
            build_app_data("", Some(" "), &[AppAttribute::AppName]),
            [&[1u8, 0, 0, 7, 0][..], b"Unknown"].concat()
        );
    }

    #[test]
    fn transfer_chunks_carry_size_running_crc_and_offset() {
        let data: Vec<u8> = (0..700u32).map(|i| (i * 7) as u8).collect();
        let mut t = Transfer::new(data.clone()).unwrap();
        let mut got = Vec::new();
        let mut offsets = Vec::new();
        let mut running = 0u16;
        while let Some(chunk) = t.next_chunk() {
            assert_eq!(u16::from_le_bytes([chunk[0], chunk[1]]), 700);
            let body = &chunk[6..];
            running = crc::update(running, body);
            assert_eq!(u16::from_le_bytes([chunk[2], chunk[3]]), running);
            offsets.push(u16::from_le_bytes([chunk[4], chunk[5]]));
            assert!(body.len() <= DATA_CHUNK);
            got.extend_from_slice(body);
        }
        assert_eq!(got, data);
        assert_eq!(offsets, vec![0, 300, 600]);
        assert!(t.done());
        assert_eq!(t.next_chunk(), None);
        assert_eq!(running, crc::checksum(&data));
    }

    #[test]
    fn transfer_refuses_empty_and_oversize_data() {
        assert_eq!(Transfer::new(Vec::new()), None);
        assert_eq!(Transfer::new(vec![0; 65_536]), None);
        assert!(Transfer::new(vec![0; 65_535]).is_some());
    }

    #[test]
    fn transfer_status_ok_is_zero() {
        assert!(transfer_ok(&[0]));
        assert!(!transfer_ok(&[1]));
        assert!(!transfer_ok(&[]));
    }

    #[test]
    fn clamping_cuts_every_text() {
        let mut n = sample();
        n.body = "x".repeat(10_000);
        n.title = "t".repeat(2_000);
        let n = n.clamped();
        assert_eq!(n.body.len(), MAX_BODY);
        assert_eq!(n.title.len(), MAX_TITLE);
        assert_eq!(n.app_id, "org.example.chat");
    }
}
