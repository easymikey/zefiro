use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    cmd::{CoverJob, ScanMode},
    domain::{
        revision::Revision,
        track::{Track, TrackRef},
    },
    message::LibraryEvent,
};

use crate::{
    cache,
    cover::decode,
    dirs::LibraryDirs,
    driver::LibraryMessage,
    error::Error,
    scan,
};

#[derive(Debug, PartialEq, Eq)]
pub enum LibraryJob {
    Cover {
        job: CoverJob,
        revision: Revision,
    },
    Tag {
        music_dir: PathBuf,
        tracks: Vec<TrackRef>,
        revision: Revision,
        dirs: Arc<LibraryDirs>,
    },
    Scan {
        music_dir: PathBuf,
        revision: Revision,
        mode: ScanMode,
        dirs: Arc<LibraryDirs>,
        decodable: &'static [&'static str],
    },
    List {
        music_dir: PathBuf,
        revision: Revision,
        decodable: &'static [&'static str],
    },
}

impl LibraryJob {
    #[must_use]
    pub fn run(self) -> LibraryMessage {
        self.read().unwrap_or_else(LibraryMessage::Error)
    }

    fn read(self) -> Result<LibraryMessage, Error> {
        match self {
            LibraryJob::Cover { job, revision } => Ok(LibraryMessage::CoverDecoded {
                revision,
                decoded: decode(job),
            }),
            LibraryJob::Tag {
                music_dir,
                tracks,
                revision,
                dirs,
            } => tagged(&dirs, &music_dir, &local_paths(tracks)).map(
                |scan::TagsRead {
                     tracks,
                     first_error,
                 }| LibraryMessage::Tagged {
                    event: LibraryEvent::Tagged { tracks, revision },
                    skipped: first_error,
                },
            ),
            LibraryJob::Scan {
                music_dir,
                revision,
                mode: ScanMode::Full,
                dirs,
                decodable,
            } => {
                let listing = listing(&music_dir, decodable)?;
                let scan::TagsRead {
                    tracks,
                    first_error,
                } = tagged(&dirs, &music_dir, &listing.paths)?;
                Ok(LibraryMessage::Scanned {
                    event: LibraryEvent::Loaded { tracks, revision },
                    skipped: listing.first_error.or(first_error),
                })
            }
            LibraryJob::Scan {
                music_dir,
                revision,
                mode: ScanMode::Cached,
                dirs,
                ..
            } => Ok(LibraryMessage::Cached {
                tracks: cache::load(&dirs, &music_dir),
                music_dir,
                revision,
            }),
            LibraryJob::List {
                music_dir,
                revision,
                decodable,
            } => {
                listing(&music_dir, decodable).map(|listing| listed(listing, revision))
            }
        }
    }

    fn priority(&self) -> JobPriority {
        match self {
            LibraryJob::Cover { .. } => JobPriority::COVER,
            LibraryJob::Tag { .. } => JobPriority::TAG,
            LibraryJob::Scan {
                mode: ScanMode::Full,
                ..
            } => JobPriority::FULL_SCAN,
            LibraryJob::Scan {
                mode: ScanMode::Cached,
                ..
            }
            | LibraryJob::List { .. } => JobPriority::CACHED_SCAN,
        }
    }

    fn scan_key(&self) -> Option<ScanKey<'_>> {
        match self {
            LibraryJob::Cover { .. } | LibraryJob::Tag { .. } => None,
            LibraryJob::Scan {
                music_dir,
                revision,
                dirs,
                decodable,
                ..
            } => Some((music_dir, revision, self.priority(), Some(dirs), decodable)),
            LibraryJob::List {
                music_dir,
                revision,
                decodable,
            } => Some((music_dir, revision, self.priority(), None, decodable)),
        }
    }
}

type ScanKey<'job> = (
    &'job PathBuf,
    &'job Revision,
    JobPriority,
    Option<&'job Arc<LibraryDirs>>,
    &'job &'static [&'static str],
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct JobPriority(u8);

impl JobPriority {
    const COVER: Self = Self(0);
    const TAG: Self = Self(1);
    const FULL_SCAN: Self = Self(2);
    const CACHED_SCAN: Self = Self(3);
}

impl PartialOrd for LibraryJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LibraryJob {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                LibraryJob::Cover { job, revision },
                LibraryJob::Cover {
                    job: other_job,
                    revision: other_revision,
                },
            ) => (&job.path, job.side, revision).cmp(&(
                &other_job.path,
                other_job.side,
                other_revision,
            )),
            (
                LibraryJob::Tag {
                    music_dir,
                    tracks,
                    revision,
                    dirs,
                },
                LibraryJob::Tag {
                    music_dir: other_dir,
                    tracks: other_tracks,
                    revision: other_revision,
                    dirs: other_dirs,
                },
            ) => (music_dir, tracks, revision, dirs).cmp(&(
                other_dir,
                other_tracks,
                other_revision,
                other_dirs,
            )),
            (
                LibraryJob::Scan { .. } | LibraryJob::List { .. },
                LibraryJob::Scan { .. } | LibraryJob::List { .. },
            ) => self.scan_key().cmp(&other.scan_key()),
            _ => self.priority().cmp(&other.priority()),
        }
    }
}

fn listed(listing: scan::Listing, revision: Revision) -> LibraryMessage {
    let scan::Listing { paths, first_error } = listing;
    LibraryMessage::Scanned {
        event: LibraryEvent::Listed {
            tracks: paths
                .iter()
                .map(|path| Arc::new(Track::listed(path)))
                .collect(),
            revision,
        },
        skipped: first_error,
    }
}

fn local_paths(tracks: Vec<TrackRef>) -> Vec<PathBuf> {
    tracks
        .into_iter()
        .map(|TrackRef::Local(path)| path)
        .collect()
}

fn tagged(
    dirs: &LibraryDirs,
    music_dir: &Path,
    paths: &[PathBuf],
) -> Result<scan::TagsRead, Error> {
    let read = scan::read_tags(paths);
    cache::save(dirs, music_dir, &read.tracks)?;
    Ok(read)
}

fn listing(music_dir: &Path, decodable: &[&str]) -> Result<scan::Listing, Error> {
    match scan::list_dir(music_dir, decodable) {
        scan::Listing {
            paths,
            first_error: Some(error),
        } if paths.is_empty() => Err(error),
        listing => Ok(listing),
    }
}
