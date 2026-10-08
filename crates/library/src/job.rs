use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    cmd::CoverJob,
    domain::{
        revision::Revision,
        track::{Track, TrackSource},
    },
};

use crate::{
    cache,
    cover::decode,
    dirs::LibraryDirs,
    error::Error,
    message::LibraryMessage,
    scan,
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LibraryJob {
    DecodeCover {
        cover_job: CoverJob,
        revision: Revision,
    },
    Tag {
        music_dir: PathBuf,
        track_sources: Vec<TrackSource>,
        revision: Revision,
        dirs: Arc<LibraryDirs>,
    },
    Scan {
        music_dir: PathBuf,
        revision: Revision,
        dirs: Arc<LibraryDirs>,
        audio_extensions: &'static [&'static str],
    },
    ReadCache {
        music_dir: PathBuf,
        revision: Revision,
        dirs: Arc<LibraryDirs>,
    },
    List {
        music_dir: PathBuf,
        revision: Revision,
        audio_extensions: &'static [&'static str],
    },
}

impl LibraryJob {
    #[must_use]
    pub fn run(self) -> LibraryMessage {
        self.read().unwrap_or_else(LibraryMessage::Error)
    }

    fn read(self) -> Result<LibraryMessage, Error> {
        match self {
            LibraryJob::DecodeCover {
                cover_job,
                revision,
            } => Ok(LibraryMessage::CoverDecoded {
                revision,
                decoded: decode(cover_job),
            }),
            LibraryJob::Tag {
                music_dir,
                track_sources,
                revision,
                dirs,
            } => {
                let scan::TagsRead { tracks, skipped } =
                    tagged(&dirs, &music_dir, &local_paths(track_sources));
                Ok(LibraryMessage::Tagged {
                    tracks,
                    revision,
                    skipped,
                })
            }
            LibraryJob::Scan {
                music_dir,
                revision,
                dirs,
                audio_extensions,
            } => {
                let listing = scan::list_dir(&music_dir, audio_extensions)?;
                let scan::TagsRead { tracks, skipped } =
                    tagged(&dirs, &music_dir, &listing.paths);
                Ok(LibraryMessage::Scanned {
                    tracks,
                    revision,
                    skipped: listing.skipped.or(skipped),
                })
            }
            LibraryJob::ReadCache {
                music_dir,
                revision,
                dirs,
            } => Ok(LibraryMessage::Cached {
                tracks: cache::load(&dirs, &music_dir),
                music_dir,
                revision,
            }),
            LibraryJob::List {
                music_dir,
                revision,
                audio_extensions,
            } => scan::list_dir(&music_dir, audio_extensions)
                .map(|listing| listed(listing, revision)),
        }
    }
}

fn listed(listing: scan::Listing, revision: Revision) -> LibraryMessage {
    let scan::Listing { paths, skipped } = listing;
    LibraryMessage::Listed {
        tracks: paths
            .iter()
            .map(|path| Arc::new(Track::listed(path)))
            .collect(),
        revision,
        skipped,
    }
}

fn local_paths(track_sources: Vec<TrackSource>) -> Vec<PathBuf> {
    track_sources
        .into_iter()
        .filter_map(|track_source| match track_source {
            TrackSource::Local(path) => Some(path),
            TrackSource::Server {
                server_name: _server_name,
                server_track_id: _server_track_id,
            } => None,
        })
        .collect()
}

fn tagged(dirs: &LibraryDirs, music_dir: &Path, paths: &[PathBuf]) -> scan::TagsRead {
    let read = scan::read_tags(paths);
    match cache::save(dirs, music_dir, &read.tracks) {
        Ok(()) => read,
        Err(error) => scan::TagsRead {
            skipped: read.skipped.or(Some(error)),
            tracks: read.tracks,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use kernel::domain::{
        revision::Revision,
        track::{Tagging, TrackSource},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use crate::{dirs::LibraryDirs, job::LibraryJob, message::LibraryMessage};

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone.wav");

    fn unwritable_cache() -> (TempDir, PathBuf, LibraryDirs) {
        let directory = tempfile::tempdir().unwrap();
        let music_dir = directory.path().join("music");
        std::fs::create_dir(&music_dir).unwrap();
        std::fs::write(music_dir.join("tone.wav"), TONE).unwrap();
        let blocker = directory.path().join("blocker");
        std::fs::write(&blocker, b"a file, not a directory").unwrap();
        let dirs = LibraryDirs {
            cache_dir: blocker.join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        (directory, music_dir, dirs)
    }

    #[test]
    fn a_failed_cache_save_keeps_the_tagged_tracks_and_reports_the_error() {
        let (_directory, music_dir, dirs) = unwritable_cache();
        let message = LibraryJob::Tag {
            track_sources: vec![TrackSource::Local(music_dir.join("tone.wav"))],
            music_dir,
            revision: Revision::default(),
            dirs: Arc::new(dirs),
        }
        .run();
        let LibraryMessage::Tagged {
            tracks, skipped, ..
        } = message
        else {
            panic!("expected tagged tracks, got {message:?}");
        };
        assert!(
            tracks
                .iter()
                .all(|track| matches!(track.tagging(), Tagging::Tagged(_))),
            "{tracks:?}"
        );
        assert!(skipped.is_some(), "the save error is reported");
    }

    #[test]
    #[cfg(unix)]
    fn an_unreadable_subfolder_alone_lists_nothing_and_reports_the_folder() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let locked = directory.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("tone.wav"), TONE).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let message = LibraryJob::List {
            music_dir: directory.path().to_path_buf(),
            revision: Revision::default(),
            audio_extensions: &["wav"],
        }
        .run();

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let LibraryMessage::Listed {
            tracks, skipped, ..
        } = message
        else {
            panic!("expected a listing, got {message:?}");
        };
        assert!(tracks.is_empty(), "{tracks:?}");
        assert!(skipped.is_some(), "the unreadable folder is reported");
    }

    #[rstest]
    #[cfg(unix)]
    #[case::a_missing_dir(None)]
    #[case::an_unreadable_dir(Some(0o000))]
    fn a_music_dir_that_cannot_be_read_answers_an_error(#[case] mode: Option<u32>) {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let music_dir = directory.path().join("music");
        if let Some(mode) = mode {
            std::fs::create_dir(&music_dir).unwrap();
            std::fs::set_permissions(&music_dir, std::fs::Permissions::from_mode(mode))
                .unwrap();
        }

        let message = LibraryJob::List {
            music_dir: music_dir.clone(),
            revision: Revision::default(),
            audio_extensions: &["wav"],
        }
        .run();

        if mode.is_some() {
            std::fs::set_permissions(
                &music_dir,
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        assert!(matches!(message, LibraryMessage::Error(_)), "{message:?}");
    }
}
