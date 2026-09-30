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
    pub(crate) bytes: Option<Vec<u8>>,
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
        let (results, arrivals) = bounded::<CoverBytes>(1);
        let handle = thread::Builder::new()
            .name("sifr-cover".to_string())
            .spawn(move || read_covers(tasks, read, &results))?;
        Ok((
            Self {
                wanted: Some(wanted),
                stale,
                handle: Some(handle),
            },
            arrivals,
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
        let bytes = cover_bytes(&track, read);
        match results.send(CoverBytes { track, bytes }) {
            Ok(()) => {}
            Err(_) => return,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Cover {
    artwork: Option<Retained<MPMediaItemArtwork>>,
}

impl Cover {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            artwork: artwork(bytes),
        }
    }

    pub(crate) fn artwork(&self) -> Option<&MPMediaItemArtwork> {
        self.artwork.as_deref()
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

fn artwork(bytes: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    let bounds = image.size();
    let handler = RcBlock::new(move |_requested: CGSize| NonNull::from(&*image));
    Some(ffi::media_item_artwork(bounds, &handler))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::OnceLock,
    };

    use crossbeam_channel::{Receiver, Sender, bounded};

    use crate::cover::{CoverWorker, artwork, cover_bytes, folder_cover};

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
        let (worker, arrivals) = CoverWorker::spawn(blocking_read).unwrap();

        worker.request(PathBuf::from("A"));
        started().1.recv().unwrap();
        worker.request(PathBuf::from("B"));
        worker.request(PathBuf::from("C"));
        gate().0.send(()).unwrap();
        let first = arrivals.recv().unwrap();

        started().1.recv().unwrap();
        gate().0.send(()).unwrap();
        let second = arrivals.recv().unwrap();

        assert_eq!(first.track, PathBuf::from("A"));
        assert_eq!(second.track, PathBuf::from("C"));
    }
}
