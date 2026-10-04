#![forbid(unsafe_code)]

use std::{
    fs,
    io,
    iter,
    path::{Path, PathBuf},
    ptr::NonNull,
};

use block2::RcBlock;
use kernel::{
    Cmd,
    MacosError,
    MacosEvent,
    domain::Revision,
    update::{Machine, Unhandled},
};
use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::NSData;
use objc2_media_player::MPMediaItemArtwork;

use crate::{
    driver::{MacosEffect, MacosMessage},
    ffi,
};

pub type CoverReader = fn(&Path) -> Option<Vec<u8>>;

const COVER_NAMES: [&str; 6] = [
    "cover.jpg",
    "cover.png",
    "folder.jpg",
    "folder.png",
    "front.jpg",
    "front.png",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverBytes {
    pub(crate) revision: Revision,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacosJob {
    ReadCover { track: PathBuf, revision: Revision },
}

impl MacosJob {
    #[must_use]
    pub fn run(self, read: CoverReader) -> MacosMessage {
        match self {
            MacosJob::ReadCover { track, revision } => {
                match cover_bytes(&track, read) {
                    Ok(bytes) => {
                        MacosMessage::CoverRead(CoverBytes { revision, bytes })
                    }
                    Err(error) => MacosMessage::Error(MacosError::Cover(error.kind())),
                }
            }
        }
    }
}

fn cover_bytes(track: &Path, read: CoverReader) -> io::Result<Vec<u8>> {
    read(track).map_or_else(
        || folder_cover(track).map_or_else(|| Ok(Vec::new()), fs::read),
        Ok,
    )
}

fn folder_cover(track: &Path) -> Option<PathBuf> {
    let folder = track.parent()?;
    COVER_NAMES
        .iter()
        .map(|name| folder.join(name))
        .find(|candidate| candidate.is_file())
}

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
            CoverMessage::Read(CoverBytes { revision, bytes })
                if revision == self.revision && !bytes.is_empty() =>
            {
                Ok([MacosEffect::ShowArtwork(bytes), MacosEffect::Publish]
                    .into_iter()
                    .collect())
            }
            CoverMessage::Read(_) => Ok(Cmd::none()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use kernel::{Cmd, MacosError, MacosEvent, domain::Revision, update::Machine};
    use rstest::rstest;

    use crate::{
        cover::{
            Cover,
            CoverBytes,
            CoverMessage,
            MacosJob,
            artwork,
            cover_bytes,
            folder_cover,
        },
        driver::{MacosEffect, MacosMessage},
    };

    fn embedded(_track: &Path) -> Option<Vec<u8>> {
        Some(b"embedded".to_vec())
    }

    fn untagged(_track: &Path) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn the_cover_beside_the_track_is_found() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("folder.png"), b"png").unwrap();
        assert_eq!(
            folder_cover(&folder.path().join("track.flac")),
            Some(folder.path().join("folder.png"))
        );
    }

    #[test]
    fn the_embedded_cover_wins_over_the_folder_cover() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("cover.jpg"), b"jpg").unwrap();
        let track = folder.path().join("track.flac");
        assert_eq!(cover_bytes(&track, embedded).unwrap(), b"embedded".to_vec());
        assert_eq!(cover_bytes(&track, untagged).unwrap(), b"jpg".to_vec());
    }

    #[test]
    fn a_track_without_any_cover_gives_no_cover() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            cover_bytes(&folder.path().join("track.flac"), untagged).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn an_unreadable_folder_cover_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let cover = folder.path().join("cover.jpg");
        fs::write(&cover, b"jpg").unwrap();
        fs::set_permissions(&cover, fs::Permissions::from_mode(0o000)).unwrap();
        let job = MacosJob::ReadCover {
            track: folder.path().join("track.flac"),
            revision: revision(1),
        };
        assert!(matches!(
            job.run(untagged),
            MacosMessage::Error(MacosError::Cover(_))
        ));
    }

    #[test]
    fn undecodable_cover_bytes_give_no_artwork() {
        assert!(artwork(b"not an image").is_none());
        assert!(artwork(b"").is_none());
    }

    #[test]
    fn the_cover_job_answers_with_its_revision() {
        let job = MacosJob::ReadCover {
            track: PathBuf::from("a.flac"),
            revision: revision(3),
        };
        let MacosMessage::CoverRead(read) = job.run(embedded) else {
            panic!("a cover job answers with CoverRead");
        };
        assert_eq!(
            read,
            CoverBytes {
                revision: revision(3),
                bytes: b"embedded".to_vec(),
            }
        );
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
            bytes: bytes.to_vec(),
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
