//! Music control: 5041 (the watch's button presses), 5042 (capabilities) and
//! 5049 (now-playing attributes pushed to the watch).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/messages/MusicControlMessage.java`,
//! `.../messages/MusicControlCapabilitiesMessage.java`,
//! `.../messages/MusicControlEntityUpdateMessage.java` (none of the three has a
//! copyright line; Gadgetbridge contributors) and `.../garmin/GarminSupport.java`
//! (`onSetMusicInfo`, `onSetMusicState`, `sendMusicState`, `onSetPhoneVolume`;
//! Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne
//! and the other Gadgetbridge contributors). *inferred (Gadgetbridge)*.
//!
//! What differs from Gadgetbridge is marked [ours]: the attributes of one 5049
//! go out in a fixed order (Gadgetbridge iterates a `HashMap`), numbers are
//! formatted with integer arithmetic (so rounding never depends on the
//! formatter), and only the first seven commands are advertised.

use super::truncate_utf8;

pub const MUSIC_CONTROL: u16 = 5041;
pub const MUSIC_CAPABILITIES: u16 = 5042;
pub const MUSIC_ENTITY_UPDATE: u16 = 5049;

/// What a watch button asks the phone's player to do (`GarminMusicControlCommand`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicCommand {
    TogglePlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Play,
    Pause,
    SkipForward,
    SkipBackward,
}

impl MusicCommand {
    /// The command number on the wire (the enum's ordinal in Gadgetbridge).
    pub fn code(self) -> u8 {
        match self {
            MusicCommand::TogglePlayPause => 0,
            MusicCommand::Next => 1,
            MusicCommand::Previous => 2,
            MusicCommand::VolumeUp => 3,
            MusicCommand::VolumeDown => 4,
            MusicCommand::Play => 5,
            MusicCommand::Pause => 6,
            MusicCommand::SkipForward => 7,
            MusicCommand::SkipBackward => 8,
        }
    }

    pub fn from_code(code: u8) -> Option<MusicCommand> {
        Some(match code {
            0 => MusicCommand::TogglePlayPause,
            1 => MusicCommand::Next,
            2 => MusicCommand::Previous,
            3 => MusicCommand::VolumeUp,
            4 => MusicCommand::VolumeDown,
            5 => MusicCommand::Play,
            6 => MusicCommand::Pause,
            7 => MusicCommand::SkipForward,
            8 => MusicCommand::SkipBackward,
            _ => return None,
        })
    }
}

/// The commands we say we handle: Gadgetbridge advertises all nine but only
/// acts on the first five; we add play and pause, which map one to one onto a
/// media controller, and leave out skip forward and back. [ours]
pub const ADVERTISED: [MusicCommand; 7] = [
    MusicCommand::TogglePlayPause,
    MusicCommand::Next,
    MusicCommand::Previous,
    MusicCommand::VolumeUp,
    MusicCommand::VolumeDown,
    MusicCommand::Play,
    MusicCommand::Pause,
];

/// 5041 payload: `u8 command`. `None` for an empty payload or a command
/// number we do not know.
pub fn parse_music_control(p: &[u8]) -> Option<MusicCommand> {
    MusicCommand::from_code(*p.first()?)
}

/// Body of the answer to 5042 (after the status byte): `u8 n` and n command
/// numbers.
pub fn capabilities_reply_body() -> Vec<u8> {
    let mut v = vec![ADVERTISED.len() as u8];
    v.extend(ADVERTISED.iter().map(|c| c.code()));
    v
}

/// One attribute of the 5049 message: an entity (0 player, 1 queue, 2 track)
/// and the attribute's number inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entity {
    pub entity: u8,
    pub attribute: u8,
}

pub const PLAYER_PLAYBACK_INFO: Entity = Entity {
    entity: 0,
    attribute: 1,
};
pub const PLAYER_VOLUME: Entity = Entity {
    entity: 0,
    attribute: 2,
};
pub const TRACK_ARTIST: Entity = Entity {
    entity: 2,
    attribute: 0,
};
pub const TRACK_ALBUM: Entity = Entity {
    entity: 2,
    attribute: 1,
};
pub const TRACK_TITLE: Entity = Entity {
    entity: 2,
    attribute: 2,
};
pub const TRACK_DURATION: Entity = Entity {
    entity: 2,
    attribute: 3,
};

/// Longest value of one attribute: the length byte holds `len + 3` and must fit
/// in a byte. Gadgetbridge throws above this; we cut at a character. [ours]
const MAX_VALUE: usize = 252;

/// 5049 payload: for each attribute `u8 len + 3`, `u8 entity`, `u8
/// attribute`, `u8 0` and the UTF-8 value.
pub fn build_entity_update(attributes: &[(Entity, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (entity, value) in attributes {
        let value = truncate_utf8(value, MAX_VALUE).as_bytes();
        out.push((value.len() + 3) as u8);
        out.push(entity.entity);
        out.push(entity.attribute);
        out.push(0);
        out.extend_from_slice(value);
    }
    out
}

/// The `PLAYBACK_INFO` value: `<playing 0|1>,<rate>,<position>`, for example
/// `1,1.0,42.000`. The rate is one decimal (half up) and 0 while paused; a rate
/// of 0 percent while playing means 1.0, as in Gadgetbridge. The position is in
/// whole seconds with three decimals.
pub fn playback_info(playing: bool, rate_percent: u16, position_s: u32) -> String {
    let rate_tenths = if !playing {
        0
    } else if rate_percent == 0 {
        10
    } else {
        (u32::from(rate_percent) + 5) / 10
    };
    format!(
        "{},{}.{},{}.000",
        u8::from(playing),
        rate_tenths / 10,
        rate_tenths % 10,
        position_s
    )
}

/// The `VOLUME` value: the phone's media volume as a fraction with two
/// decimals, for example `0.50`. Above 100 percent counts as 100.
pub fn volume_value(percent: u8) -> String {
    let p = u32::from(percent.min(100));
    format!("{}.{:02}", p / 100, p % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_and_unknown_codes_are_refused() {
        for code in 0..=8u8 {
            let c = MusicCommand::from_code(code).unwrap();
            assert_eq!(c.code(), code);
        }
        assert_eq!(MusicCommand::from_code(9), None);
        assert_eq!(parse_music_control(&[]), None);
        assert_eq!(parse_music_control(&[1]), Some(MusicCommand::Next));
    }

    #[test]
    fn capabilities_body_lists_the_advertised_commands() {
        assert_eq!(capabilities_reply_body(), vec![7, 0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn entity_update_layout() {
        let b = build_entity_update(&[
            (TRACK_TITLE, "Hi".to_string()),
            (TRACK_DURATION, "180".to_string()),
        ]);
        assert_eq!(
            b,
            vec![5, 2, 2, 0, b'H', b'i', 6, 2, 3, 0, b'1', b'8', b'0']
        );
    }

    #[test]
    fn long_values_are_cut_to_fit_the_length_byte() {
        let b = build_entity_update(&[(TRACK_ARTIST, "é".repeat(200))]);
        assert_eq!(b[0] as usize, b.len() - 1);
        // 252 bytes of two-byte characters: exactly 126 of them
        assert_eq!(b.len() - 4, 252);
        assert!(String::from_utf8(b[4..].to_vec()).is_ok());
    }

    #[test]
    fn playback_info_strings() {
        assert_eq!(playback_info(true, 100, 42), "1,1.0,42.000");
        assert_eq!(playback_info(true, 0, 0), "1,1.0,0.000");
        assert_eq!(playback_info(false, 100, 7), "0,0.0,7.000");
        assert_eq!(playback_info(true, 150, 1), "1,1.5,1.000");
        // 125 percent is a tie at one decimal; half up gives 1.3 (Java's %.1f)
        assert_eq!(playback_info(true, 125, 1), "1,1.3,1.000");
        assert_eq!(playback_info(true, 200, 1), "1,2.0,1.000");
    }

    #[test]
    fn volume_strings() {
        assert_eq!(volume_value(0), "0.00");
        assert_eq!(volume_value(5), "0.05");
        assert_eq!(volume_value(50), "0.50");
        assert_eq!(volume_value(100), "1.00");
        assert_eq!(volume_value(255), "1.00");
    }
}
