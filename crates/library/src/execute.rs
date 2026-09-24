use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    LibraryCmd,
    LoadedRequest,
    Message,
    Track,
    domain::Revision,
    playlist::PlaylistFileName,
};

use crate::{
    cache,
    error::LibraryError,
    favorites,
    history,
    paths::LibraryPaths,
    playlists,
    scan::{self, Skips},
    trash,
};

pub fn execute(
    cmd: LibraryCmd,
    paths: &LibraryPaths,
    decodable: &[&str],
) -> Result<Option<Message>, LibraryError> {
    match cmd {
        LibraryCmd::AppendHistory { track, .. } => append_history(paths, &track),
        LibraryCmd::SaveFavorites(favorites) => save_favorites(paths, &favorites),
        LibraryCmd::LoadFavorites => load_favorites(paths),
        LibraryCmd::Trash(path) => trash_track(&path),
        LibraryCmd::LoadHistory { limit } => load_history(paths, limit),
        LibraryCmd::Rescan { root, revision } => rescan(
            paths,
            &ScanRequest {
                root: &root,
                revision,
            },
            decodable,
        ),
        LibraryCmd::SavePlaylist { name, tracks } => {
            save_playlist(paths, &name, &tracks)
        }
        LibraryCmd::ScanLibrary { root, revision } => scan_library(
            paths,
            &ScanRequest {
                root: &root,
                revision,
            },
            decodable,
        ),
        LibraryCmd::PrefetchCover(_) => Ok(None),
        LibraryCmd::TagTracks {
            root,
            paths: listed,
            revision,
        } => tag_tracks(
            paths,
            TagRequest {
                root,
                listed,
                revision,
            },
        ),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn append_history(
    paths: &LibraryPaths,
    track: &Track,
) -> Result<Option<Message>, LibraryError> {
    history::append(paths, track, unix_now())?;
    Ok(None)
}

fn save_favorites(
    paths: &LibraryPaths,
    favorites: &HashSet<PathBuf>,
) -> Result<Option<Message>, LibraryError> {
    favorites::save(paths, favorites)?;
    Ok(None)
}

fn load_favorites(paths: &LibraryPaths) -> Result<Option<Message>, LibraryError> {
    let loaded = favorites::load(paths)?;
    Ok(Some(Message::Loaded(LoadedRequest::FavoritesLoaded(
        loaded,
    ))))
}

fn trash_track(path: &Path) -> Result<Option<Message>, LibraryError> {
    trash::move_to_trash(path)?;
    Ok(None)
}

fn load_history(
    paths: &LibraryPaths,
    limit: usize,
) -> Result<Option<Message>, LibraryError> {
    let entries = history::read(paths, limit)?;
    Ok(Some(Message::Loaded(LoadedRequest::HistoryLoaded(entries))))
}

#[derive(Clone, Copy)]
struct ScanRequest<'a> {
    root: &'a Path,
    revision: Revision,
}

fn rescan(
    paths: &LibraryPaths,
    request: &ScanRequest<'_>,
    decodable: &[&str],
) -> Result<Option<Message>, LibraryError> {
    let ScanRequest { root, revision } = *request;
    let report = scan::scan_dir(root, decodable);
    if let Some(error) = empty_scan_error(report.tracks.len(), report.skipped) {
        return Err(error);
    }
    cache::save(paths, root, &report.tracks)?;
    Ok(Some(Message::Loaded(LoadedRequest::LibraryLoaded {
        tracks: report.tracks,
        revision,
    })))
}

fn empty_scan_error(found: usize, skipped: Skips) -> Option<LibraryError> {
    skipped.first_error.filter(|_| found == 0)
}

fn save_playlist(
    paths: &LibraryPaths,
    name: &str,
    tracks: &[Arc<Track>],
) -> Result<Option<Message>, LibraryError> {
    let Ok(name) = PlaylistFileName::new(name) else {
        return Ok(None);
    };
    playlists::save(paths, &name, tracks)?;
    Ok(None)
}

fn scan_library(
    paths: &LibraryPaths,
    request: &ScanRequest<'_>,
    decodable: &[&str],
) -> Result<Option<Message>, LibraryError> {
    let ScanRequest { root, revision } = *request;
    if let Some(cached) = cache::load(paths, root) {
        return Ok(Some(Message::Loaded(LoadedRequest::LibraryLoaded {
            tracks: cached,
            revision,
        })));
    }
    let listing = scan::list_dir(root, decodable);
    if let Some(error) = empty_scan_error(listing.paths.len(), listing.skipped) {
        return Err(error);
    }
    let tracks: Vec<Arc<Track>> = listing
        .paths
        .iter()
        .map(|path| Arc::new(Track::listed(path)))
        .collect();
    Ok(Some(Message::Loaded(LoadedRequest::LibraryListed {
        tracks,
        revision,
    })))
}

struct TagRequest {
    root: PathBuf,
    listed: Vec<PathBuf>,
    revision: Revision,
}

fn tag_tracks(
    paths: &LibraryPaths,
    request: TagRequest,
) -> Result<Option<Message>, LibraryError> {
    let TagRequest {
        root,
        listed,
        revision,
    } = request;
    let tracks = backfilled(&listed, scan::tag_tracks(&listed).tracks);
    cache::save(paths, &root, &tracks)?;
    Ok(Some(Message::Loaded(LoadedRequest::TracksTagged {
        tracks,
        revision,
    })))
}

fn backfilled(paths: &[PathBuf], read: Vec<Arc<Track>>) -> Vec<Arc<Track>> {
    if read.len() == paths.len() {
        return read;
    }
    let mut unread = read.into_iter().peekable();
    paths
        .iter()
        .map(|path| {
            unread
                .next_if(|track| track.path() == *path)
                .unwrap_or_else(|| Arc::new(Track::listed(path)))
        })
        .collect()
}
