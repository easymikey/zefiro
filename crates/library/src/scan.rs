use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{domain::track::Track, message::LibrarySubject};

use crate::{error::Error, tags::read_track};

#[must_use]
fn is_audio_file(path: &Path, audio_extensions: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            audio_extensions
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) skipped: Option<Error>,
}

impl Listing {
    fn keeping(mut self, walked: walkdir::DirEntry, audio_extensions: &[&str]) -> Self {
        if is_audio_file(walked.path(), audio_extensions) && walked.path().is_file() {
            self.paths.push(walked.into_path());
        }
        self
    }

    fn skipping(mut self, error: walkdir::Error, music_dir: &Path) -> Self {
        if self.skipped.is_none() {
            self.skipped = Some(walk_error(error, music_dir));
        }
        self
    }
}

fn walk_error(error: walkdir::Error, music_dir: &Path) -> Error {
    let path = error
        .path()
        .map_or_else(|| music_dir.to_path_buf(), Path::to_path_buf);
    let text = error.to_string();
    let io_error = error
        .into_io_error()
        .unwrap_or_else(|| std::io::Error::other(text));
    Error::io(LibrarySubject::Scan, &path)(io_error)
}

pub(crate) fn list_dir(
    music_dir: &Path,
    audio_extensions: &[&str],
) -> Result<Listing, Error> {
    if !music_dir.is_dir() {
        let kind = if music_dir.exists() {
            std::io::ErrorKind::NotADirectory
        } else {
            std::io::ErrorKind::NotFound
        };
        return Err(Error::io(LibrarySubject::Scan, music_dir)(
            std::io::Error::from(kind),
        ));
    }
    walkdir::WalkDir::new(music_dir)
        .sort_by_file_name()
        .into_iter()
        .try_fold(Listing::default(), |listing, entry| match entry {
            Ok(entry) => Ok(listing.keeping(entry, audio_extensions)),
            Err(error) if error.depth() == 0 => Err(walk_error(error, music_dir)),
            Err(error) => Ok(listing.skipping(error, music_dir)),
        })
}

#[derive(Debug, Default)]
pub(crate) struct TagsRead {
    pub(crate) tracks: Vec<Arc<Track>>,
    pub(crate) skipped: Option<Error>,
}

impl TagsRead {
    fn adding(mut self, path: &Path) -> Self {
        match read_track(path) {
            Ok(track) => self.tracks.push(Arc::new(track)),
            Err(error) => {
                self.tracks.push(Arc::new(Track::listed(path)));
                self.skipped = self.skipped.or(Some(error));
            }
        }
        self
    }

    fn joined(mut self, tags_read: TagsRead) -> Self {
        self.tracks.extend(tags_read.tracks);
        self.skipped = self.skipped.or(tags_read.skipped);
        self
    }
}

fn read_chunk(chunk_paths: &[PathBuf]) -> TagsRead {
    chunk_paths
        .iter()
        .fold(TagsRead::default(), |read, path| read.adding(path))
}

pub(crate) fn read_tags(paths: &[PathBuf]) -> TagsRead {
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
            .map(|(chunk, handle)| chunk_read(chunk, handle.join()))
            .fold(TagsRead::default(), TagsRead::joined)
    })
}

fn chunk_read(
    chunk_paths: &[PathBuf],
    joined: std::thread::Result<TagsRead>,
) -> TagsRead {
    joined.unwrap_or_else(|_panicked| TagsRead {
        tracks: chunk_paths
            .iter()
            .map(|path| Arc::new(Track::listed(path)))
            .collect(),
        skipped: Some(Error::io(
            LibrarySubject::Scan,
            chunk_paths.first().map_or(Path::new(""), PathBuf::as_path),
        )(std::io::Error::other("tag reader panicked"))),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    use kernel::domain::track::{Tagging, Track};
    use rstest::{fixture, rstest};

    use crate::{
        error::Error,
        scan::{chunk_read, is_audio_file, list_dir, read_tags},
        test_support::temp_dir_filters,
    };

    const AUDIO_EXTENSIONS: &[&str] =
        &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

    #[rstest]
    #[case("song.flac")]
    #[case("song.m4b")]
    #[case("song.MP3")]
    #[case("song.mkv")]
    fn an_audio_extension_counts_as_audio(#[case] name: &str) {
        assert!(
            is_audio_file(Path::new(name), AUDIO_EXTENSIONS),
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
    fn an_unlisted_extension_is_not_audio(#[case] name: &str) {
        assert!(
            !is_audio_file(Path::new(name), AUDIO_EXTENSIONS),
            "{name} cannot be decoded, so it must not count as audio"
        );
    }

    #[rstest]
    #[case("notes.txt")]
    #[case("cover.jpg")]
    #[case("README")]
    fn anything_else_is_passed_over(#[case] name: &str) {
        assert!(
            !is_audio_file(Path::new(name), AUDIO_EXTENSIONS),
            "{name} is not audio"
        );
    }

    fn scanned(dir: &Path) -> Vec<Arc<Track>> {
        read_tags(&list_dir(dir, AUDIO_EXTENSIONS).unwrap().paths).tracks
    }

    #[test]
    fn a_panicked_tag_thread_lists_its_chunk_and_reports_an_error() {
        let chunk = [
            PathBuf::from("/music/a.flac"),
            PathBuf::from("/music/b.mp3"),
        ];

        let read = chunk_read(&chunk, Err(Box::new("tag reader panicked")));

        assert_eq!(read.tracks.len(), 2);
        assert!(read.skipped.is_some(), "the panic is reported");
    }

    #[fixture]
    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[rstest]
    fn listing_keeps_audio_files_sorted_and_skips_others(temp_dir: tempfile::TempDir) {
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
        let listing = list_dir(temp_dir.path(), AUDIO_EXTENSIONS).unwrap();
        assert!(listing.skipped.is_none());
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
        let listing = list_dir(temp_dir.path(), AUDIO_EXTENSIONS).unwrap();
        let chunk = &listing.paths[..2];

        let tracks = read_tags(chunk).tracks;

        assert_eq!(tracks.len(), 2);
        assert!(
            tracks
                .iter()
                .all(|track| matches!(track.tagging(), Tagging::Listed(_)))
        );
        assert_eq!(
            tracks
                .iter()
                .map(|track| track.local_path().unwrap().to_path_buf())
                .collect::<Vec<_>>(),
            chunk
        );
    }

    #[test]
    fn tagging_no_paths_reads_nothing() {
        assert!(read_tags(&[]).tracks.is_empty());
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

        let skipped = list_dir(&music_dir, AUDIO_EXTENSIONS).err();

        insta::with_settings!({ filters => temp_dir_filters(), snapshot_suffix => name.unwrap_or("gone") }, {
            insta::assert_debug_snapshot!(skipped);
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

        let listing = list_dir(temp_dir.path(), AUDIO_EXTENSIONS).unwrap();

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .unwrap();

        assert_eq!(listing.paths.len(), 1);
        let skipped = listing.skipped.as_ref();
        assert!(
            skipped.is_some_and(|error| matches!(
                error,
                Error::Io { path, .. } if path.as_os_str() != ""
            )),
            "the walk error must keep a non-empty path"
        );
        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(listing.skipped);
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
                .all(|track| matches!(track.tagging(), Tagging::Listed(_)))
        );
    }

    #[rstest]
    fn a_directory_named_like_audio_is_not_listed(temp_dir: tempfile::TempDir) {
        let folder = temp_dir.path().join("Live.flac");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("one.mp3"), b"stub").unwrap();

        let listing = list_dir(temp_dir.path(), AUDIO_EXTENSIONS).unwrap();

        assert_eq!(listing.paths, vec![folder.join("one.mp3")]);
    }

    #[rstest]
    fn a_directory_of_readable_tracks_skips_nothing(temp_dir: tempfile::TempDir) {
        for name in ["a.flac", "b.mp3"] {
            std::fs::write(temp_dir.path().join(name), b"stub").unwrap();
        }
        assert_eq!(scanned(temp_dir.path()).len(), 2);
    }
}
