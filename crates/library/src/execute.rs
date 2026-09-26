use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    LibraryCmd,
    LibraryFact,
    Track,
    domain::Revision,
    playlist::PlaylistFileName,
};

use crate::{
    cache::{self, CacheMiss},
    error::LibraryError,
    favorites,
    history,
    paths::LibraryPaths,
    playlists,
    scan::{self, ScanReport, Skips},
    trash,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryNote {
    CacheMissed(CacheMiss),
    HistoryLinesSkipped { lines: usize },
    ScanEntriesSkipped { entries: usize },
}

#[derive(Debug, Default, PartialEq)]
pub struct Executed {
    pub fact: Option<LibraryFact>,
    pub notes: Vec<LibraryNote>,
}

fn answered(fact: LibraryFact) -> Executed {
    Executed {
        fact: Some(fact),
        notes: Vec::new(),
    }
}

pub fn execute(
    command: LibraryCmd,
    paths: &LibraryPaths,
    decodable: &[&str],
) -> Result<Executed, LibraryError> {
    match command {
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
        LibraryCmd::PrefetchCover(_) => Ok(Executed::default()),
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
) -> Result<Executed, LibraryError> {
    history::append(paths, track, unix_now())?;
    Ok(Executed::default())
}

fn save_favorites(
    paths: &LibraryPaths,
    favorites: &HashSet<PathBuf>,
) -> Result<Executed, LibraryError> {
    favorites::save(paths, favorites)?;
    Ok(Executed::default())
}

fn load_favorites(paths: &LibraryPaths) -> Result<Executed, LibraryError> {
    let loaded = favorites::load(paths)?;
    Ok(answered(LibraryFact::FavoritesLoaded(loaded)))
}

fn trash_track(path: &Path) -> Result<Executed, LibraryError> {
    trash::move_to_trash(path)?;
    Ok(Executed::default())
}

fn load_history(paths: &LibraryPaths, limit: usize) -> Result<Executed, LibraryError> {
    let history::HistoryRead {
        entries,
        skipped_lines,
    } = history::read(paths, limit)?;
    let mut executed = answered(LibraryFact::HistoryLoaded(entries));
    if skipped_lines > 0 {
        executed.notes.push(LibraryNote::HistoryLinesSkipped {
            lines: skipped_lines,
        });
    }
    Ok(executed)
}

#[derive(Clone, Copy)]
struct ScanRequest<'a> {
    root: &'a Path,
    revision: Revision,
}

fn empty_scan_error(
    found: usize,
    first_error: Option<LibraryError>,
) -> Option<LibraryError> {
    first_error.filter(|_| found == 0)
}

fn skipped_note(found: usize, skipped: usize) -> Option<LibraryNote> {
    (found > 0 && skipped > 0)
        .then_some(LibraryNote::ScanEntriesSkipped { entries: skipped })
}

fn rescan(
    paths: &LibraryPaths,
    request: &ScanRequest<'_>,
    decodable: &[&str],
) -> Result<Executed, LibraryError> {
    let ScanRequest { root, revision } = *request;
    let ScanReport { tracks, skipped } = scan::scan_dir(root, decodable);
    let Skips { count, first_error } = skipped;
    if let Some(error) = empty_scan_error(tracks.len(), first_error) {
        return Err(error);
    }
    cache::save(paths, root, &tracks)?;
    let note = skipped_note(tracks.len(), count);
    let mut executed = answered(LibraryFact::Loaded { tracks, revision });
    executed.notes.extend(note);
    Ok(executed)
}

fn save_playlist(
    paths: &LibraryPaths,
    name: &PlaylistFileName,
    tracks: &[Arc<Track>],
) -> Result<Executed, LibraryError> {
    playlists::save(paths, name, tracks)?;
    Ok(Executed::default())
}

fn scan_library(
    paths: &LibraryPaths,
    request: &ScanRequest<'_>,
    decodable: &[&str],
) -> Result<Executed, LibraryError> {
    let ScanRequest { root, revision } = *request;
    let miss = match cache::load(paths, root) {
        Ok(cached) => {
            return Ok(answered(LibraryFact::Loaded {
                tracks: cached,
                revision,
            }));
        }
        Err(miss) => miss,
    };
    let listing = scan::list_dir(root, decodable);
    let Skips { count, first_error } = listing.skipped;
    if let Some(error) = empty_scan_error(listing.paths.len(), first_error) {
        return Err(error);
    }
    let tracks: Vec<Arc<Track>> = listing
        .paths
        .iter()
        .map(|path| Arc::new(Track::listed(path)))
        .collect();
    let mut notes = Vec::new();
    if miss != CacheMiss::Absent {
        notes.push(LibraryNote::CacheMissed(miss));
    }
    notes.extend(skipped_note(tracks.len(), count));
    Ok(Executed {
        fact: Some(LibraryFact::Listed { tracks, revision }),
        notes,
    })
}

struct TagRequest {
    root: PathBuf,
    listed: Vec<PathBuf>,
    revision: Revision,
}

fn tag_tracks(
    paths: &LibraryPaths,
    request: TagRequest,
) -> Result<Executed, LibraryError> {
    let TagRequest {
        root,
        listed,
        revision,
    } = request;
    let tracks = backfilled(&listed, scan::tag_tracks(&listed).tracks);
    cache::save(paths, &root, &tracks)?;
    Ok(answered(LibraryFact::Tagged { tracks, revision }))
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
