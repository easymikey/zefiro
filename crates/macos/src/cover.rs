#![forbid(unsafe_code)]

use std::{iter, path::PathBuf, ptr::NonNull};

use block2::RcBlock;
use kernel::{
    cmd::Cmd,
    domain::revision::Revision,
    message::MacosEvent,
    update::machine::{Machine, Unhandled},
};
use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::NSData;
use objc2_media_player::MPMediaItemArtwork;

use crate::{effect::MacosEffect, ffi, job::MacosJob, message::CoverBytes};

pub(crate) fn artwork(bytes: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
    if bytes.is_empty() {
        return None;
    }
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    let bounds = image.size();
    let handler = RcBlock::new(move |_requested: CGSize| NonNull::from(&*image));
    Some(ffi::media_item_artwork(bounds, &handler))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Cover {
    track: Option<PathBuf>,
    revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverMessage {
    TrackShown(Option<PathBuf>),
    Read(CoverBytes),
}

impl Machine for Cover {
    type Message = CoverMessage;
    type Effect = Cmd<MacosEffect, MacosEvent>;

    fn transition(&mut self, message: CoverMessage) -> Result<Self::Effect, Unhandled> {
        match message {
            CoverMessage::TrackShown(track) if self.track == track => Ok(Cmd::none()),
            CoverMessage::TrackShown(track) => {
                self.track.clone_from(&track);
                self.revision = self.revision.next();
                let revision = self.revision;
                let read = track.map(|track| {
                    MacosEffect::Run(MacosJob::ReadCover { track, revision })
                });
                Ok(iter::once(MacosEffect::ClearArtwork).chain(read).collect())
            }
            CoverMessage::Read(CoverBytes { revision, .. })
                if revision != self.revision =>
            {
                Ok(Cmd::none())
            }
            CoverMessage::Read(CoverBytes {
                bytes: Ok(bytes), ..
            }) if bytes.is_empty() => Ok(Cmd::none()),
            CoverMessage::Read(CoverBytes {
                bytes: Ok(bytes), ..
            }) => Ok([MacosEffect::ShowArtwork(bytes), MacosEffect::Publish]
                .into_iter()
                .collect()),
            CoverMessage::Read(CoverBytes {
                bytes: Err(error), ..
            }) => Ok(Cmd::message(MacosEvent::Error(error))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use kernel::{
        cmd::Cmd,
        domain::revision::Revision,
        message::{MacosError, MacosEvent},
        update::machine::Machine,
    };
    use rstest::rstest;

    use crate::{
        cover::{Cover, CoverMessage, artwork},
        effect::MacosEffect,
        job::MacosJob,
        message::CoverBytes,
    };

    #[test]
    fn undecodable_cover_bytes_give_no_artwork() {
        assert!(artwork(b"not an image").is_none());
        assert!(artwork(b"").is_none());
    }

    fn track(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn revision(count: u8) -> Revision {
        (0..count).fold(Revision::default(), |revision, _| revision.next())
    }

    fn holding(name: Option<&str>, count: u8) -> Cover {
        Cover {
            track: name.map(track),
            revision: revision(count),
        }
    }

    fn read(name: &str, count: u8) -> MacosEffect {
        MacosEffect::Run(MacosJob::ReadCover {
            track: track(name),
            revision: revision(count),
        })
    }

    fn bytes_of(count: u8, bytes: &[u8]) -> CoverMessage {
        CoverMessage::Read(CoverBytes {
            revision: revision(count),
            bytes: Ok(bytes.to_vec()),
        })
    }

    fn unreadable() -> MacosError {
        MacosError::Cover(io::ErrorKind::PermissionDenied.into())
    }

    fn failed(count: u8) -> CoverMessage {
        CoverMessage::Read(CoverBytes {
            revision: revision(count),
            bytes: Err(unreadable()),
        })
    }

    struct Row {
        cover: Cover,
        message: CoverMessage,
        next: Cover,
        cmd: Cmd<MacosEffect, MacosEvent>,
    }

    #[rstest]
    #[case::first_track_reads_its_cover(Row {
        cover: Cover::default(),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: holding(Some("a.flac"), 1),
        cmd: [MacosEffect::ClearArtwork, read("a.flac", 1)].into_iter().collect(),
    })]
    #[case::the_same_track_keeps_it(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: holding(Some("a.flac"), 1),
        cmd: Cmd::none(),
    })]
    #[case::a_new_track_clears_and_reads(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(Some(track("b.flac"))),
        next: holding(Some("b.flac"), 2),
        cmd: [MacosEffect::ClearArtwork, read("b.flac", 2)].into_iter().collect(),
    })]
    #[case::the_cover_read_shows_it(Row {
        cover: holding(Some("a.flac"), 1),
        message: bytes_of(1, b"art"),
        next: holding(Some("a.flac"), 1),
        cmd: [MacosEffect::ShowArtwork(b"art".to_vec()), MacosEffect::Publish]
            .into_iter()
            .collect(),
    })]
    #[case::a_stale_cover_read_is_ignored(Row {
        cover: holding(Some("b.flac"), 2),
        message: bytes_of(1, b"art"),
        next: holding(Some("b.flac"), 2),
        cmd: Cmd::none(),
    })]
    #[case::a_failed_read_is_reported(Row {
        cover: holding(Some("a.flac"), 1),
        message: failed(1),
        next: holding(Some("a.flac"), 1),
        cmd: Cmd::message(MacosEvent::Error(unreadable())),
    })]
    #[case::a_stale_failed_read_is_ignored(Row {
        cover: holding(Some("b.flac"), 2),
        message: failed(1),
        next: holding(Some("b.flac"), 2),
        cmd: Cmd::none(),
    })]
    #[case::cleared_clears(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(None),
        next: holding(None, 2),
        cmd: Cmd::effect(MacosEffect::ClearArtwork),
    })]
    #[case::a_track_without_cover_shows_nothing(Row {
        cover: holding(Some("a.flac"), 1),
        message: bytes_of(1, b""),
        next: holding(Some("a.flac"), 1),
        cmd: Cmd::none(),
    })]
    fn the_cover_slot_reads_clears_or_shows_by_the_revision_it_holds(#[case] row: Row) {
        let Row {
            mut cover,
            message,
            next,
            cmd,
        } = row;
        assert_eq!(cover.transition(message), Ok(cmd));
        assert_eq!(cover, next);
    }
}
