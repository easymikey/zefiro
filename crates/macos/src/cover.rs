#![forbid(unsafe_code)]

use std::{
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
    ptr::NonNull,
    thread::{self, JoinHandle},
};

use block2::RcBlock;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::NSData;
use objc2_media_player::MPMediaItemArtwork;

use crate::ffi;

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
pub(crate) struct CoverBytes {
    pub(crate) track: PathBuf,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct CoverWorker {
    wanted: Option<Sender<PathBuf>>,
    stale: Receiver<PathBuf>,
    handle: Option<JoinHandle<()>>,
}

impl CoverWorker {
    pub(crate) fn spawn(
        read: CoverReader,
    ) -> Result<(Self, Receiver<CoverBytes>), io::Error> {
        let (wanted, tasks) = bounded::<PathBuf>(1);
        let stale = tasks.clone();
        let (results, covers_read) = bounded::<CoverBytes>(1);
        let handle = thread::Builder::new()
            .name("sifr-cover".to_string())
            .spawn(move || read_covers(tasks, read, &results))?;
        Ok((
            Self {
                wanted: Some(wanted),
                stale,
                handle: Some(handle),
            },
            covers_read,
        ))
    }

    pub(crate) fn request(&self, track: PathBuf) {
        let Some(wanted) = &self.wanted else {
            return;
        };
        if let Err(TrySendError::Full(track)) = wanted.try_send(track) {
            match self.stale.try_recv() {
                Ok(_) | Err(_) => {}
            }
            match wanted.try_send(track) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

impl fmt::Debug for CoverWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoverWorker")
            .finish_non_exhaustive()
    }
}

impl Drop for CoverWorker {
    fn drop(&mut self) {
        self.wanted = None;
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

fn read_covers(
    tasks: Receiver<PathBuf>,
    read: CoverReader,
    results: &Sender<CoverBytes>,
) {
    for track in tasks {
        let bytes = cover_bytes(&track, read).unwrap_or_default();
        match results.send(CoverBytes { track, bytes }) {
            Ok(()) => {}
            Err(_) => return,
        }
    }
}

fn cover_bytes(track: &Path, read: CoverReader) -> Option<Vec<u8>> {
    read(track).or_else(|| folder_cover(track).and_then(|cover| fs::read(cover).ok()))
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
pub(crate) struct CoverState {
    track: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverMessage {
    TrackShown(Option<PathBuf>),
    Read(CoverBytes),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverEffect {
    Nothing,
    Clear,
    Request(PathBuf),
    Show(Vec<u8>),
}

impl CoverState {
    pub(crate) fn apply(&mut self, message: CoverMessage) -> CoverEffect {
        match message {
            CoverMessage::TrackShown(track) => self.track_shown(track),
            CoverMessage::Read(bytes) => self.cover_read(bytes),
        }
    }

    fn track_shown(&mut self, track: Option<PathBuf>) -> CoverEffect {
        if self.track == track {
            return CoverEffect::Nothing;
        }
        self.track.clone_from(&track);
        track.map_or(CoverEffect::Clear, CoverEffect::Request)
    }

    fn cover_read(&self, bytes: CoverBytes) -> CoverEffect {
        if self.track.as_deref() == Some(bytes.track.as_path()) {
            CoverEffect::Show(bytes.bytes)
        } else {
            CoverEffect::Nothing
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::OnceLock,
    };

    use crossbeam_channel::{Receiver, Sender, bounded};
    use rstest::rstest;

    use crate::cover::{
        CoverBytes,
        CoverEffect,
        CoverMessage,
        CoverState,
        CoverWorker,
        artwork,
        cover_bytes,
        folder_cover,
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
        assert_eq!(cover_bytes(&track, embedded), Some(b"embedded".to_vec()));
        assert_eq!(cover_bytes(&track, untagged), Some(b"jpg".to_vec()));
    }

    #[test]
    fn a_track_without_any_cover_gives_no_cover() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            cover_bytes(&folder.path().join("track.flac"), untagged),
            None
        );
    }

    #[test]
    fn undecodable_cover_bytes_give_no_artwork() {
        assert!(artwork(b"not an image").is_none());
        assert!(artwork(b"").is_none());
    }

    fn started() -> &'static (Sender<()>, Receiver<()>) {
        static STARTED: OnceLock<(Sender<()>, Receiver<()>)> = OnceLock::new();
        STARTED.get_or_init(|| bounded(0))
    }

    fn gate() -> &'static (Sender<()>, Receiver<()>) {
        static GATE: OnceLock<(Sender<()>, Receiver<()>)> = OnceLock::new();
        GATE.get_or_init(|| bounded(0))
    }

    fn blocking_read(_track: &Path) -> Option<Vec<u8>> {
        match started().0.send(()) {
            Ok(()) | Err(_) => {}
        }
        match gate().1.recv() {
            Ok(()) | Err(_) => {}
        }
        Some(b"cover".to_vec())
    }

    #[test]
    fn the_latest_wanted_track_wins() {
        let (worker, covers_read) = CoverWorker::spawn(blocking_read).unwrap();

        worker.request(PathBuf::from("A"));
        started().1.recv().unwrap();
        worker.request(PathBuf::from("B"));
        worker.request(PathBuf::from("C"));
        gate().0.send(()).unwrap();
        let first = covers_read.recv().unwrap();

        started().1.recv().unwrap();
        gate().0.send(()).unwrap();
        let second = covers_read.recv().unwrap();

        assert_eq!(first.track, PathBuf::from("A"));
        assert_eq!(second.track, PathBuf::from("C"));
    }

    fn track(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    struct Row {
        cover: CoverState,
        message: CoverMessage,
        next: CoverState,
        effect: CoverEffect,
    }

    #[rstest]
    #[case::first_track_requests_its_cover(Row {
        cover: CoverState::default(),
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: CoverState { track: Some(track("a.flac")) },
        effect: CoverEffect::Request(track("a.flac")),
    })]
    #[case::the_same_track_keeps_it(Row {
        cover: CoverState { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(Some(track("a.flac"))),
        next: CoverState { track: Some(track("a.flac")) },
        effect: CoverEffect::Nothing,
    })]
    #[case::a_new_track_clears_and_requests(Row {
        cover: CoverState { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(Some(track("b.flac"))),
        next: CoverState { track: Some(track("b.flac")) },
        effect: CoverEffect::Request(track("b.flac")),
    })]
    #[case::the_cover_read_shows_it(Row {
        cover: CoverState { track: Some(track("a.flac")) },
        message: CoverMessage::Read(CoverBytes {
            track: track("a.flac"),
            bytes: b"art".to_vec(),
        }),
        next: CoverState { track: Some(track("a.flac")) },
        effect: CoverEffect::Show(b"art".to_vec()),
    })]
    #[case::a_stale_cover_read_is_ignored(Row {
        cover: CoverState { track: Some(track("b.flac")) },
        message: CoverMessage::Read(CoverBytes {
            track: track("a.flac"),
            bytes: b"art".to_vec(),
        }),
        next: CoverState { track: Some(track("b.flac")) },
        effect: CoverEffect::Nothing,
    })]
    #[case::cleared_clears(Row {
        cover: CoverState { track: Some(track("a.flac")) },
        message: CoverMessage::TrackShown(None),
        next: CoverState { track: None },
        effect: CoverEffect::Clear,
    })]
    #[case::a_track_without_cover_shows_nothing(Row {
        cover: CoverState { track: Some(track("a.flac")) },
        message: CoverMessage::Read(CoverBytes {
            track: track("a.flac"),
            bytes: Vec::new(),
        }),
        next: CoverState { track: Some(track("a.flac")) },
        effect: CoverEffect::Show(Vec::new()),
    })]
    fn the_cover_slot_requests_clears_or_shows_by_the_track_it_holds(#[case] row: Row) {
        let Row {
            mut cover,
            message,
            next,
            effect,
        } = row;
        let observed_effect = cover.apply(message);
        assert_eq!(cover, next);
        assert_eq!(observed_effect, effect);
    }
}
