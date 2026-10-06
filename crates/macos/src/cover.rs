#![forbid(unsafe_code)]

use std::{
    io,
    iter,
    path::{Path, PathBuf},
    ptr::NonNull,
};

use block2::RcBlock;
use kernel::{
    cmd::Cmd,
    domain::revision::Revision,
    message::{MacosError, MacosEvent},
    update::machine::{LoopEffect, Machine, Unhandled},
};
use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::NSData;
use objc2_media_player::MPMediaItemArtwork;

use crate::{
    effect::MacosEffect,
    ffi,
    job::{MacosJob, MacosLoopCmd},
    message::CoverBytes,
};

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

impl Cover {
    pub(crate) fn shows(&self, path: Option<&Path>) -> bool {
        self.track.as_deref() == path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverMessage {
    TrackShown(Option<PathBuf>),
    Read(CoverBytes),
}

impl Machine for Cover {
    type Message = CoverMessage;
    type Effect = MacosLoopCmd;

    fn transition(&mut self, message: CoverMessage) -> Result<Self::Effect, Unhandled> {
        match message {
            CoverMessage::TrackShown(track) if self.track == track => Err(Unhandled),
            CoverMessage::TrackShown(track) => {
                self.track.clone_from(&track);
                self.revision = self.revision.next();
                let revision = self.revision;
                let read = track.map(|track| {
                    LoopEffect::Run(MacosJob::ReadCover { track, revision })
                });
                Ok(iter::once(LoopEffect::Execute(MacosEffect::ClearArtwork))
                    .chain(read)
                    .collect())
            }
            CoverMessage::Read(CoverBytes { revision, .. })
                if revision != self.revision =>
            {
                Err(Unhandled)
            }
            CoverMessage::Read(CoverBytes {
                bytes: Ok(bytes), ..
            }) if bytes.is_empty() => Err(Unhandled),
            CoverMessage::Read(CoverBytes {
                bytes: Err(MacosError::ReadArtwork(error)),
                ..
            }) if error == io::ErrorKind::NotFound.into() => Err(Unhandled),
            CoverMessage::Read(CoverBytes {
                bytes: Ok(bytes), ..
            }) => Ok([MacosEffect::ShowArtwork(bytes), MacosEffect::Publish]
                .into_iter()
                .map(LoopEffect::Execute)
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
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{Cover, CoverMessage, artwork},
        effect::MacosEffect,
        job::{MacosJob, MacosLoopCmd},
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

    fn read(name: &str, count: u8) -> MacosJob {
        MacosJob::ReadCover {
            track: track(name),
            revision: revision(count),
        }
    }

    type Placed = (Vec<MacosEffect>, Vec<MacosJob>, Vec<MacosEvent>);

    fn placed(loop_cmd: MacosLoopCmd) -> Placed {
        let (effects, events) = loop_cmd.into_parts();
        let (executed, jobs) = effects.into_iter().fold(
            (Vec::new(), Vec::new()),
            |(executed, jobs), effect| match effect {
                LoopEffect::Execute(effect) => {
                    ([executed, vec![effect]].concat(), jobs)
                }
                LoopEffect::Run(job) => (executed, [jobs, vec![job]].concat()),
                other @ (LoopEffect::After { .. }
                | LoopEffect::Watch { .. }
                | LoopEffect::Unwatch(_)) => panic!("not placed: {other:?}"),
            },
        );
        (executed, jobs, events)
    }

    fn bytes_of(count: u8, bytes: &[u8]) -> CoverMessage {
        CoverMessage::Read(CoverBytes {
            revision: revision(count),
            bytes: Ok(bytes.to_vec()),
        })
    }

    fn unreadable() -> MacosError {
        MacosError::ReadArtwork(io::ErrorKind::PermissionDenied.into())
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
        cmd: Result<Cmd<MacosEffect, MacosEvent>, Unhandled>,
        jobs: Vec<MacosJob>,
    }

    #[rstest]
    #[case::first_track_reads_its_cover(Row {
        cover: Cover::default(),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: holding(Some("a.flac"), 1),
        cmd: Ok(Cmd::effect(MacosEffect::ClearArtwork)),
        jobs: vec![read("a.flac", 1)],
    })]
    #[case::the_same_track_is_refused(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: holding(Some("a.flac"), 1),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_new_track_clears_and_reads(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(Some(track("b.flac"))),
        next: holding(Some("b.flac"), 2),
        cmd: Ok(Cmd::effect(MacosEffect::ClearArtwork)),
        jobs: vec![read("b.flac", 2)],
    })]
    #[case::the_cover_read_shows_it(Row {
        cover: holding(Some("a.flac"), 1),
        message: bytes_of(1, b"art"),
        next: holding(Some("a.flac"), 1),
        cmd: Ok([MacosEffect::ShowArtwork(b"art".to_vec()), MacosEffect::Publish]
            .into_iter()
            .collect()),
        jobs: vec![],
    })]
    #[case::a_stale_cover_read_is_refused(Row {
        cover: holding(Some("b.flac"), 2),
        message: bytes_of(1, b"art"),
        next: holding(Some("b.flac"), 2),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_failed_read_is_reported(Row {
        cover: holding(Some("a.flac"), 1),
        message: failed(1),
        next: holding(Some("a.flac"), 1),
        cmd: Ok(Cmd::message(MacosEvent::Error(unreadable()))),
        jobs: vec![],
    })]
    #[case::a_stale_failed_read_is_refused(Row {
        cover: holding(Some("b.flac"), 2),
        message: failed(1),
        next: holding(Some("b.flac"), 2),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_track_with_no_cover_found_shows_nothing(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::Read(CoverBytes {
            revision: revision(1),
            bytes: Err(MacosError::ReadArtwork(io::ErrorKind::NotFound.into())),
        }),
        next: holding(Some("a.flac"), 1),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::cleared_clears(Row {
        cover: holding(Some("a.flac"), 1),
        message: CoverMessage::TrackShown(None),
        next: holding(None, 2),
        cmd: Ok(Cmd::effect(MacosEffect::ClearArtwork)),
        jobs: vec![],
    })]
    #[case::a_track_without_cover_shows_nothing(Row {
        cover: holding(Some("a.flac"), 1),
        message: bytes_of(1, b""),
        next: holding(Some("a.flac"), 1),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    fn the_cover_slot_reads_clears_or_shows_by_the_revision_it_holds(#[case] row: Row) {
        let Row {
            mut cover,
            message,
            next,
            cmd,
            jobs,
        } = row;
        let expected = cmd.map(|cmd| {
            let (effects, events) = cmd.into_parts();
            (effects, jobs, events)
        });
        assert_eq!(cover.transition(message).map(placed), expected);
        assert_eq!(cover, next);
    }
}
