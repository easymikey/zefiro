#![forbid(unsafe_code)]

use std::{io, iter, ptr::NonNull, sync::Arc};

use block2::RcBlock;
use kernel::{
    cmd::Cmd,
    domain::{revision::Revision, track::Track},
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
    message::ArtworkBytes,
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

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Artwork {
    track: Option<Arc<Track>>,
    revision: Revision,
}

impl Artwork {
    pub(crate) fn show_track(&mut self, track: Option<Arc<Track>>) -> MacosLoopCmd {
        if self.track.as_deref().map(Track::source)
            == track.as_deref().map(Track::source)
        {
            return Cmd::none();
        }
        self.revision = self.revision.next();
        let revision = self.revision;
        let read = track.as_deref().and_then(Track::local_path).map(|path| {
            LoopEffect::Run(MacosJob::ReadArtwork {
                path: path.to_path_buf(),
                revision,
            })
        });
        self.track = track;
        iter::once(LoopEffect::Execute(MacosEffect::ClearArtwork))
            .chain(read)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ArtworkMessage {
    ReadDone(ArtworkBytes),
}

impl Machine for Artwork {
    type Message = ArtworkMessage;
    type Effect = MacosLoopCmd;

    fn transition(
        &mut self,
        message: ArtworkMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match message {
            ArtworkMessage::ReadDone(ArtworkBytes { revision, bytes }) => {
                if revision != self.revision {
                    return Err(Unhandled);
                }
                match bytes {
                    Ok(bytes) if bytes.is_empty() => Err(Unhandled),
                    Ok(bytes) => Ok([
                        MacosEffect::ShowArtwork(bytes),
                        MacosEffect::ShowNowPlaying,
                    ]
                    .into_iter()
                    .map(LoopEffect::Execute)
                    .collect()),
                    Err(MacosError::ReadArtwork(error))
                        if error == io::ErrorKind::NotFound.into() =>
                    {
                        Err(Unhandled)
                    }
                    Err(error) => Ok(Cmd::message(MacosEvent::Error(error))),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::Path, sync::Arc};

    use kernel::{
        cmd::Cmd,
        domain::{revision::Revision, track::Track},
        message::{MacosError, MacosEvent},
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        artwork::{Artwork, ArtworkMessage, artwork},
        effect::MacosEffect,
        job::{MacosJob, MacosLoopCmd},
        message::ArtworkBytes,
    };

    #[test]
    fn undecodable_artwork_bytes_give_no_artwork() {
        assert!(artwork(b"not an image").is_none());
        assert!(artwork(b"").is_none());
    }

    fn track(name: &str) -> Arc<Track> {
        Arc::new(Track::listed(Path::new(name)))
    }

    fn revision(count: u8) -> Revision {
        (0..count).fold(Revision::default(), |revision, _| revision.next())
    }

    fn holding(name: Option<&str>, count: u8) -> Artwork {
        Artwork {
            track: name.map(track),
            revision: revision(count),
        }
    }

    fn read(name: &str, count: u8) -> MacosJob {
        MacosJob::ReadArtwork {
            path: name.into(),
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

    fn bytes_of(count: u8, bytes: &[u8]) -> ArtworkMessage {
        ArtworkMessage::ReadDone(ArtworkBytes {
            revision: revision(count),
            bytes: Ok(bytes.to_vec()),
        })
    }

    fn unreadable() -> MacosError {
        MacosError::ReadArtwork(io::ErrorKind::PermissionDenied.into())
    }

    fn failed(count: u8) -> ArtworkMessage {
        ArtworkMessage::ReadDone(ArtworkBytes {
            revision: revision(count),
            bytes: Err(unreadable()),
        })
    }

    struct Row {
        artwork: Artwork,
        message: ArtworkMessage,
        next: Artwork,
        cmd: Result<Cmd<MacosEffect, MacosEvent>, Unhandled>,
        jobs: Vec<MacosJob>,
    }

    struct ShowRow {
        artwork: Artwork,
        track: Option<Arc<Track>>,
        next: Artwork,
        cmd: Cmd<MacosEffect, MacosEvent>,
        jobs: Vec<MacosJob>,
    }

    #[rstest]
    #[case::first_track_reads_its_artwork(ShowRow {
        artwork: Artwork::default(),
        track: Some(track("a.flac")),
        next: holding(Some("a.flac"), 1),
        cmd: Cmd::effect(MacosEffect::ClearArtwork),
        jobs: vec![read("a.flac", 1)],
    })]
    #[case::the_same_track_reads_nothing(ShowRow {
        artwork: holding(Some("a.flac"), 1),
        track: Some(track("a.flac")),
        next: holding(Some("a.flac"), 1),
        cmd: Cmd::none(),
        jobs: vec![],
    })]
    #[case::a_new_track_clears_and_reads(ShowRow {
        artwork: holding(Some("a.flac"), 1),
        track: Some(track("b.flac")),
        next: holding(Some("b.flac"), 2),
        cmd: Cmd::effect(MacosEffect::ClearArtwork),
        jobs: vec![read("b.flac", 2)],
    })]
    #[case::cleared_clears(ShowRow {
        artwork: holding(Some("a.flac"), 1),
        track: None,
        next: holding(None, 2),
        cmd: Cmd::effect(MacosEffect::ClearArtwork),
        jobs: vec![],
    })]
    fn a_shown_track_clears_and_reads_its_artwork_unless_already_shown(
        #[case] show_row: ShowRow,
    ) {
        let ShowRow {
            mut artwork,
            track,
            next,
            cmd,
            jobs,
        } = show_row;
        let (effects, events) = cmd.into_parts();
        assert_eq!(placed(artwork.show_track(track)), (effects, jobs, events));
        assert_eq!(artwork, next);
    }

    #[rstest]
    #[case::the_artwork_read_shows_it(Row {
        artwork: holding(Some("a.flac"), 1),
        message: bytes_of(1, b"art"),
        next: holding(Some("a.flac"), 1),
        cmd: Ok([MacosEffect::ShowArtwork(b"art".to_vec()), MacosEffect::ShowNowPlaying]
            .into_iter()
            .collect()),
        jobs: vec![],
    })]
    #[case::a_stale_artwork_read_is_refused(Row {
        artwork: holding(Some("b.flac"), 2),
        message: bytes_of(1, b"art"),
        next: holding(Some("b.flac"), 2),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_failed_read_is_reported(Row {
        artwork: holding(Some("a.flac"), 1),
        message: failed(1),
        next: holding(Some("a.flac"), 1),
        cmd: Ok(Cmd::message(MacosEvent::Error(unreadable()))),
        jobs: vec![],
    })]
    #[case::a_stale_failed_read_is_refused(Row {
        artwork: holding(Some("b.flac"), 2),
        message: failed(1),
        next: holding(Some("b.flac"), 2),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_track_with_no_artwork_found_shows_nothing(Row {
        artwork: holding(Some("a.flac"), 1),
        message: ArtworkMessage::ReadDone(ArtworkBytes {
            revision: revision(1),
            bytes: Err(MacosError::ReadArtwork(io::ErrorKind::NotFound.into())),
        }),
        next: holding(Some("a.flac"), 1),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    #[case::a_track_without_artwork_shows_nothing(Row {
        artwork: holding(Some("a.flac"), 1),
        message: bytes_of(1, b""),
        next: holding(Some("a.flac"), 1),
        cmd: Err(Unhandled),
        jobs: vec![],
    })]
    fn the_artwork_slot_reads_clears_or_shows_by_the_revision_it_holds(
        #[case] row: Row,
    ) {
        let Row {
            mut artwork,
            message,
            next,
            cmd,
            jobs,
        } = row;
        let expected = cmd.map(|cmd| {
            let (effects, events) = cmd.into_parts();
            (effects, jobs, events)
        });
        assert_eq!(artwork.transition(message).map(placed), expected);
        assert_eq!(artwork, next);
    }
}
