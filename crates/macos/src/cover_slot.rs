#![forbid(unsafe_code)]

use std::path::PathBuf;

use kernel::update::{Machine, Rejected};

use crate::cover::CoverBytes;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CoverSlot {
    track: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverMessage {
    TrackShown(Option<PathBuf>),
    Arrived(CoverBytes),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverEffect {
    Nothing,
    Clear,
    Request(PathBuf),
    Show(Vec<u8>),
}

impl Machine for CoverSlot {
    type Message = CoverMessage;
    type Error = std::convert::Infallible;
    type Effect = CoverEffect;

    fn transition(
        self,
        message: CoverMessage,
    ) -> Result<(Self, CoverEffect), Rejected<Self>> {
        Ok(match message {
            CoverMessage::TrackShown(track) => track_shown(self, track),
            CoverMessage::Arrived(bytes) => arrived(self, bytes),
        })
    }
}

fn track_shown(slot: CoverSlot, track: Option<PathBuf>) -> (CoverSlot, CoverEffect) {
    if slot.track == track {
        (slot, CoverEffect::Nothing)
    } else {
        track.map_or((CoverSlot { track: None }, CoverEffect::Clear), |wanted| {
            (
                CoverSlot {
                    track: Some(wanted.clone()),
                },
                CoverEffect::Request(wanted),
            )
        })
    }
}

fn arrived(slot: CoverSlot, bytes: CoverBytes) -> (CoverSlot, CoverEffect) {
    if slot.track.as_deref() == Some(bytes.track.as_path()) {
        let effect = bytes.bytes.map_or(CoverEffect::Clear, CoverEffect::Show);
        (slot, effect)
    } else {
        (slot, CoverEffect::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::update::Machine;
    use rstest::rstest;

    use crate::{
        cover::CoverBytes,
        cover_slot::{CoverEffect, CoverMessage, CoverSlot},
    };

    fn track(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    struct Row {
        slot: CoverSlot,
        message: CoverMessage,
        next: CoverSlot,
        effect: CoverEffect,
    }

    #[rstest]
    #[case::first_track_requests_its_cover(Row {
        slot: CoverSlot::default(),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: CoverSlot { track: Some(track("a.flac")) },
        effect: CoverEffect::Request(track("a.flac")),
    })]
    #[case::the_same_track_keeps_it(Row {
        slot: CoverSlot { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: CoverSlot { track: Some(track("a.flac")) },
        effect: CoverEffect::Nothing,
    })]
    #[case::a_new_track_clears_and_requests(Row {
        slot: CoverSlot { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(Some(track("b.flac"))),
        next: CoverSlot { track: Some(track("b.flac")) },
        effect: CoverEffect::Request(track("b.flac")),
    })]
    #[case::the_arrival_shows_it(Row {
        slot: CoverSlot { track: Some(track("a.flac")) },
        message: CoverMessage::Arrived(CoverBytes {
            track: track("a.flac"),
            bytes: Some(b"art".to_vec()),
        }),
        next: CoverSlot { track: Some(track("a.flac")) },
        effect: CoverEffect::Show(b"art".to_vec()),
    })]
    #[case::a_stale_arrival_is_ignored(Row {
        slot: CoverSlot { track: Some(track("b.flac")) },
        message: CoverMessage::Arrived(CoverBytes {
            track: track("a.flac"),
            bytes: Some(b"art".to_vec()),
        }),
        next: CoverSlot { track: Some(track("b.flac")) },
        effect: CoverEffect::Nothing,
    })]
    #[case::cleared_clears(Row {
        slot: CoverSlot { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(None),
        next: CoverSlot { track: None },
        effect: CoverEffect::Clear,
    })]
    #[case::a_track_without_cover_clears(Row {
        slot: CoverSlot { track: Some(track("a.flac")) },
        message: CoverMessage::Arrived(CoverBytes {
            track: track("a.flac"),
            bytes: None,
        }),
        next: CoverSlot { track: Some(track("a.flac")) },
        effect: CoverEffect::Clear,
    })]
    fn the_cover_slot_requests_clears_or_shows_by_the_track_it_holds(#[case] row: Row) {
        let Row {
            mut slot,
            message,
            next,
            effect,
        } = row;
        let Ok(observed_effect) = slot.update(message);
        assert_eq!(slot, next);
        assert_eq!(observed_effect, effect);
    }
}
