use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{LibrarySubject, Track};

use crate::{error::Error, tags::read_or_list};

#[must_use]
fn is_decodable(path: &Path, decodable: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| decodable.contains(&extension.as_str()))
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub paths: Vec<PathBuf>,
    pub first_error: Option<Error>,
}

impl Listing {
    fn keeping(mut self, walked: walkdir::DirEntry, decodable: &[&str]) -> Self {
        if is_decodable(walked.path(), decodable) {
            self.paths.push(walked.into_path());
        }
        self
    }

    fn skipping(mut self, error: walkdir::Error, music_dir: &Path) -> Self {
        if self.first_error.is_none() {
            self.first_error = Some(walk_error(error, music_dir));
        }
        self
    }
}

fn walk_error(error: walkdir::Error, music_dir: &Path) -> Error {
    let path = error
        .path()
        .map_or_else(|| music_dir.to_path_buf(), Path::to_path_buf);
    let text = error.to_string();
    let source = error
        .into_io_error()
        .unwrap_or_else(|| std::io::Error::other(text));
    Error::io(LibrarySubject::Scan, &path)(source)
}

pub(crate) fn list_dir(music_dir: &Path, decodable: &[&str]) -> Listing {
    if !music_dir.is_dir() {
        let kind = if music_dir.exists() {
            std::io::ErrorKind::NotADirectory
        } else {
            std::io::ErrorKind::NotFound
        };
        return Listing {
            paths: Vec::new(),
            first_error: Some(Error::io(LibrarySubject::Scan, music_dir)(
                std::io::Error::from(kind),
            )),
        };
    }
    walkdir::WalkDir::new(music_dir)
        .sort_by_file_name()
        .into_iter()
        .fold(Listing::default(), |listing, entry| match entry {
            Ok(entry) => listing.keeping(entry, decodable),
            Err(err) => listing.skipping(err, music_dir),
        })
}

fn read_chunk(chunk: &[PathBuf]) -> Vec<Arc<Track>> {
    chunk.iter().map(|path| read_or_list(path)).collect()
}

pub(crate) fn read_tags(paths: &[PathBuf]) -> Vec<Arc<Track>> {
    let workers =
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    if workers <= 1 || paths.len() <= 1 {
        return read_chunk(paths);
    }
    let chunk_size = paths.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk_size)
            .map(|chunk| (chunk, scope.spawn(|| read_chunk(chunk))))
            .collect();
        handles
            .into_iter()
            .flat_map(|(chunk, handle)| {
                handle.join().unwrap_or_else(|_| {
                    chunk
                        .iter()
                        .map(|path| Arc::new(Track::listed(path)))
                        .collect()
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use kernel::{Tagging, Track};
    use rstest::{fixture, rstest};

    use crate::{
        error::Error,
        scan::{is_decodable, list_dir, read_tags},
        test_support::temp_dir_filters,
    };

    const DECODABLE: &[&str] =
        &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

    #[rstest]
    #[case("song.flac")]
    #[case("song.m4b")]
    #[case("song.MP3")]
    #[case("song.mkv")]
    fn a_decodable_extension_counts_as_audio(#[case] name: &str) {
        assert!(
            is_decodable(Path::new(name), DECODABLE),
            "{name} should count as audio"
        );
    }

    #[rstest]
    #[case("song.aiff")]
    #[case("song.wv")]
    #[case("song.mpc")]
    #[case("song.ape")]
    #[case("song.opus")]
    #[case("clip.webm")]
    #[case("clip.WEBM")]
    fn an_undecodable_extension_is_not_audio(#[case] name: &str) {
        assert!(
            !is_decodable(Path::new(name), DECODABLE),
            "{name} cannot be decoded, so it must not count as audio"
        );
    }

    #[rstest]
    #[case("notes.txt")]
    #[case("cover.jpg")]
    #[case("README")]
    fn anything_else_is_passed_over(#[case] name: &str) {
        assert!(
            !is_decodable(Path::new(name), DECODABLE),
            "{name} is not audio"
        );
    }

    fn scanned(dir: &Path) -> Vec<Arc<Track>> {
        read_tags(&list_dir(dir, DECODABLE).paths)
    }

    #[fixture]
    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[rstest]
    fn listing_keeps_decodable_files_sorted_and_skips_others(
        temp_dir: tempfile::TempDir,
    ) {
        for name in ["b.mp3", "a.flac", "x.txt", "c.mkv"] {
            std::fs::write(temp_dir.path().join(name), b"stub").unwrap();
        }
        let tracks = scanned(temp_dir.path());
        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(tracks);
        });
    }

    #[rstest]
    fn listing_names_the_audio_files_before_a_single_tag_is_read(
        temp_dir: tempfile::TempDir,
    ) {
        for name in ["b.mp3", "a.flac", "notes.txt"] {
            std::fs::write(temp_dir.path().join(name), b"stub").unwrap();
        }
        let listing = list_dir(temp_dir.path(), DECODABLE);
        assert!(listing.first_error.is_none());
        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(listing.paths);
        });
    }

    #[rstest]
    fn a_chunk_of_paths_with_unparseable_containers_stays_listed_not_tagged(
        temp_dir: tempfile::TempDir,
    ) {
        for name in ["a.flac", "b.mp3", "c.mkv"] {
            std::fs::write(temp_dir.path().join(name), b"stub").unwrap();
        }
        let listing = list_dir(temp_dir.path(), DECODABLE);
        let chunk = &listing.paths[..2];

        let tracks = read_tags(chunk);

        assert_eq!(tracks.len(), 2);
        assert!(
            tracks
                .iter()
                .all(|track| track.tagging() == Tagging::Listed)
        );
        assert_eq!(
            tracks
                .iter()
                .map(|track| track.path().to_path_buf())
                .collect::<Vec<_>>(),
            chunk
        );
    }

    #[test]
    fn tagging_no_paths_reads_nothing() {
        assert!(read_tags(&[]).is_empty());
    }

    #[rstest]
    #[case::a_file_where_a_directory_was_asked_for(Some("one.mp3"))]
    #[case::nothing_at_that_path(None)]
    fn a_music_dir_that_is_not_a_directory_lists_nothing_and_says_why(
        temp_dir: tempfile::TempDir,
        #[case] name: Option<&str>,
    ) {
        let music_dir = temp_dir.path().join(name.unwrap_or("gone"));
        if name.is_some() {
            std::fs::write(&music_dir, b"stub").unwrap();
        }

        let listing = list_dir(&music_dir, DECODABLE);

        assert!(listing.paths.is_empty());
        insta::with_settings!({ filters => temp_dir_filters(), snapshot_suffix => name.unwrap_or("gone") }, {
            insta::assert_debug_snapshot!(listing.first_error);
        });
    }

    #[rstest]
    #[cfg(unix)]
    fn scan_lists_readable_entries_and_keeps_the_first_walk_error(
        temp_dir: tempfile::TempDir,
    ) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(temp_dir.path().join("a.mp3"), b"stub").unwrap();
        let locked = temp_dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("b.mp3"), b"stub").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let listing = list_dir(temp_dir.path(), DECODABLE);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .unwrap();

        assert_eq!(listing.paths.len(), 1);
        let first_error = listing.first_error.as_ref();
        assert!(
            first_error.is_some_and(|error| matches!(
                error,
                Error::Io { path, .. } if path.as_os_str() != ""
            )),
            "the walk error must keep a non-empty path"
        );
        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(listing.first_error);
        });
    }

    #[rstest]
    #[cfg(unix)]
    fn a_track_whose_tags_cannot_be_read_stays_listed(temp_dir: tempfile::TempDir) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(temp_dir.path().join("readable.mp3"), b"stub").unwrap();
        let sealed = temp_dir.path().join("sealed.mp3");
        std::fs::write(&sealed, b"stub").unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let tracks = scanned(temp_dir.path());

        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o644))
            .unwrap();

        assert_eq!(tracks.len(), 2);
        assert!(
            tracks
                .iter()
                .all(|track| track.tagging() == Tagging::Listed)
        );
    }

    #[rstest]
    fn a_directory_of_readable_tracks_skips_nothing(temp_dir: tempfile::TempDir) {
        for name in ["a.flac", "b.mp3"] {
            std::fs::write(temp_dir.path().join(name), b"stub").unwrap();
        }
        assert_eq!(scanned(temp_dir.path()).len(), 2);
    }
}
