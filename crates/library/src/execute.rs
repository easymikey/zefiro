use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    LibraryCmd,
    LibraryEvent,
    Track,
    domain::{Revision, ScanMode},
};

use crate::{
    cache::{self, CacheMiss},
    dirs::LibraryDirs,
    error::Error,
    favorites,
    history,
    playlists,
    scan::{self, ScanReport, Skipped},
    trash,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryWarning {
    CacheMissed(CacheMiss),
    HistoryLinesSkipped { lines: usize },
    ScanEntriesSkipped { entries: usize },
}

#[derive(Debug, Default, PartialEq)]
pub struct Executed {
    pub event: Option<LibraryEvent>,
    pub warnings: Vec<LibraryWarning>,
}

fn answered(event: LibraryEvent) -> Executed {
    Executed {
        event: Some(event),
        warnings: Vec::new(),
    }
}

pub fn execute(
    command: LibraryCmd,
    dirs: &LibraryDirs,
    decodable: &[&str],
) -> Result<Executed, Error> {
    match command {
        LibraryCmd::AppendHistory { track, at } => {
            history::append(dirs, &track, at).map(|()| Executed::default())
        }
        LibraryCmd::SaveFavorites(favorites) => {
            favorites::save(dirs, &favorites).map(|()| Executed::default())
        }
        LibraryCmd::LoadFavorites => load_favorites(dirs),
        LibraryCmd::Trash(path) => {
            trash::move_to_trash(&path).map(|()| Executed::default())
        }
        LibraryCmd::LoadHistory { limit } => load_history(dirs, limit),
        LibraryCmd::Scan {
            music_dir,
            revision,
            mode,
        } => {
            let request = ScanParts {
                music_dir: &music_dir,
                revision,
            };
            match mode {
                ScanMode::Full => scan_full(dirs, &request, decodable),
                ScanMode::Cached => scan_cached(dirs, &request, decodable),
            }
        }
        LibraryCmd::SavePlaylist { name, tracks } => {
            playlists::save(dirs, &name, &tracks).map(|()| Executed::default())
        }
        LibraryCmd::PrefetchCover(_) => Ok(Executed::default()),
        LibraryCmd::TagTracks {
            music_dir,
            paths: listed,
            revision,
        } => tag_tracks(
            dirs,
            TagJob {
                music_dir,
                listed,
                revision,
            },
        ),
    }
}

fn load_favorites(dirs: &LibraryDirs) -> Result<Executed, Error> {
    let loaded = favorites::load(dirs)?;
    Ok(answered(LibraryEvent::FavoritesLoaded(loaded)))
}

fn load_history(dirs: &LibraryDirs, limit: usize) -> Result<Executed, Error> {
    let history::LoadedHistory {
        entries,
        skipped_lines,
    } = history::load(dirs, limit)?;
    Ok(Executed {
        event: Some(LibraryEvent::HistoryLoaded(entries)),
        warnings: (skipped_lines > 0)
            .then_some(LibraryWarning::HistoryLinesSkipped {
                lines: skipped_lines,
            })
            .into_iter()
            .collect(),
    })
}

#[derive(Clone, Copy)]
struct ScanParts<'a> {
    music_dir: &'a Path,
    revision: Revision,
}

fn empty_scan_error(found: usize, first_error: Option<Error>) -> Option<Error> {
    first_error.filter(|_| found == 0)
}

fn skipped_warning(found: usize, skipped: usize) -> Option<LibraryWarning> {
    (found > 0 && skipped > 0)
        .then_some(LibraryWarning::ScanEntriesSkipped { entries: skipped })
}

fn scan_full(
    dirs: &LibraryDirs,
    request: &ScanParts<'_>,
    decodable: &[&str],
) -> Result<Executed, Error> {
    let ScanParts {
        music_dir,
        revision,
    } = *request;
    let ScanReport { tracks, skipped } = scan::scan_dir(music_dir, decodable);
    let Skipped { count, first_error } = skipped;
    if let Some(error) = empty_scan_error(tracks.len(), first_error) {
        return Err(error);
    }
    cache::save(dirs, music_dir, &tracks)?;
    let warnings = skipped_warning(tracks.len(), count).into_iter().collect();
    Ok(Executed {
        event: Some(LibraryEvent::Loaded { tracks, revision }),
        warnings,
    })
}

fn scan_cached(
    dirs: &LibraryDirs,
    request: &ScanParts<'_>,
    decodable: &[&str],
) -> Result<Executed, Error> {
    let ScanParts {
        music_dir,
        revision,
    } = *request;
    let miss = match cache::load(dirs, music_dir) {
        Ok(cached) => {
            return Ok(answered(LibraryEvent::Loaded {
                tracks: cached,
                revision,
            }));
        }
        Err(miss) => miss,
    };
    let listing = scan::list_dir(music_dir, decodable);
    let Skipped { count, first_error } = listing.skipped;
    if let Some(error) = empty_scan_error(listing.paths.len(), first_error) {
        return Err(error);
    }
    let tracks: Vec<Arc<Track>> = listing
        .paths
        .iter()
        .map(|path| Arc::new(Track::listed(path)))
        .collect();
    let warnings = [
        (miss != CacheMiss::Missing).then_some(LibraryWarning::CacheMissed(miss)),
        skipped_warning(tracks.len(), count),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(Executed {
        event: Some(LibraryEvent::Listed { tracks, revision }),
        warnings,
    })
}

struct TagJob {
    music_dir: PathBuf,
    listed: Vec<PathBuf>,
    revision: Revision,
}

fn tag_tracks(dirs: &LibraryDirs, request: TagJob) -> Result<Executed, Error> {
    let TagJob {
        music_dir,
        listed,
        revision,
    } = request;
    let tracks = backfilled(&listed, scan::read_tags(&listed).tracks);
    cache::save(dirs, &music_dir, &tracks)?;
    Ok(answered(LibraryEvent::Tagged { tracks, revision }))
}

fn backfilled(paths: &[PathBuf], read: Vec<Arc<Track>>) -> Vec<Arc<Track>> {
    if read.len() == paths.len() {
        return read;
    }
    let mut read_tracks = read.into_iter().peekable();
    paths
        .iter()
        .map(|path| {
            read_tracks
                .next_if(|track| track.path() == *path)
                .unwrap_or_else(|| Arc::new(Track::listed(path)))
        })
        .collect()
}
