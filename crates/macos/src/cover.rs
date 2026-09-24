#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    ptr::NonNull,
};

use block2::RcBlock;
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

#[derive(Debug)]
pub(crate) struct Cover {
    track: PathBuf,
    artwork: Option<Retained<MPMediaItemArtwork>>,
}

impl Cover {
    pub(crate) fn new(track: &Path, read: CoverReader) -> Self {
        Self {
            track: track.to_path_buf(),
            artwork: cover_bytes(track, read).and_then(|bytes| artwork(&bytes)),
        }
    }

    pub(crate) fn track(&self) -> &Path {
        &self.track
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
    use std::{fs, path::Path};

    use crate::cover::{artwork, cover_bytes, folder_cover};

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
}
