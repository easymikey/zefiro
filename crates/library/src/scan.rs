use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    domain::{io_error::IoError, overlay::Verdict, track::Track},
    message::LibrarySubject,
};

use crate::{error::Error, tags::read_track};

#[must_use]
pub fn probe(path: &Path) -> Verdict {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::read_dir(path)
            .map_or_else(|error| refused(&error), |_| Verdict::Readable),
        Ok(_) => Verdict::NotADirectory,
        Err(error) => refused(&error),
    }
}

fn refused(error: &std::io::Error) -> Verdict {
    if error.kind() == std::io::ErrorKind::NotADirectory {
        return Verdict::NotADirectory;
    }
    match IoError::from(error.kind()) {
        IoError::Missing => Verdict::Missing,
        IoError::Denied => Verdict::Denied,
        unreadable @ (IoError::Malformed | IoError::Full | IoError::Other) => {
            Verdict::Unreadable(unreadable)
        }
    }
}

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
    use std::path::{Path, PathBuf};

    use kernel::domain::track::Tagging;
    use rstest::{fixture, rstest};

    use crate::{
        error::Error,
        scan::{chunk_read, is_audio_file, list_dir, probe, read_tags},
        test_support::temp_dir_filters,
    };

    #[derive(Clone, Copy)]
    enum Probed {
        Folder,
        File,
        Gone,
        Locked,
        ThroughAFile,
        Looped,
    }

    #[rstest]
    #[case::a_folder_reads(Probed::Folder, kernel::domain::overlay::Verdict::Readable)]
    #[case::a_file_is_not_a_folder(
        Probed::File,
        kernel::domain::overlay::Verdict::NotADirectory
    )]
    #[case::a_missing_path(Probed::Gone, kernel::domain::overlay::Verdict::Missing)]
    #[case::a_folder_without_permission(
        Probed::Locked,
        kernel::domain::overlay::Verdict::Denied
    )]
    #[case::a_path_through_a_file_is_not_a_folder(
        Probed::ThroughAFile,
        kernel::domain::overlay::Verdict::NotADirectory
    )]
    #[case::a_link_loop_cannot_be_read(
        Probed::Looped,
        kernel::domain::overlay::Verdict::Unreadable(
            kernel::domain::io_error::IoError::Other
        )
    )]
    fn a_probe_names_what_the_path_is(
        #[case] probed: Probed,
        #[case] expected: kernel::domain::overlay::Verdict,
    ) {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        let path = match probed {
            Probed::ThroughAFile => music.join("inner"),
            Probed::Folder
            | Probed::File
            | Probed::Gone
            | Probed::Locked
            | Probed::Looped => music.clone(),
        };
        match probed {
            Probed::Folder => std::fs::create_dir(&path).unwrap(),
            Probed::File => std::fs::write(&path, b"").unwrap(),
            Probed::Gone => {}
            Probed::ThroughAFile => std::fs::write(&music, b"").unwrap(),
            Probed::Looped => std::os::unix::fs::symlink(&music, &music).unwrap(),
            Probed::Locked => {
                std::fs::create_dir(&path).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
                    .unwrap();
            }
        }
        let verdict = probe(&path);
        std::fs::set_permissions(
            directory.path(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        if path.is_dir() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        assert_eq!(verdict, expected);
    }

    const AUDIO_EXTENSIONS: &[&str] =
        &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

    #[rstest]
    #[case("song.flac")]
    #[case("song.MP3")]
    fn an_audio_extension_counts_as_audio(#[case] name: &str) {
        assert!(
            is_audio_file(Path::new(name), AUDIO_EXTENSIONS),
            "{name} should count as audio"
        );
    }

    #[rstest]
    #[case("song.opus")]
    #[case("notes.txt")]
    #[case("README")]
    fn anything_else_is_passed_over(#[case] name: &str) {
        assert!(
            !is_audio_file(Path::new(name), AUDIO_EXTENSIONS),
            "{name} is not audio"
        );
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
    fn a_directory_named_like_audio_is_not_listed(temp_dir: tempfile::TempDir) {
        let folder = temp_dir.path().join("Live.flac");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("one.mp3"), b"stub").unwrap();

        let listing = list_dir(temp_dir.path(), AUDIO_EXTENSIONS).unwrap();

        assert_eq!(listing.paths, vec![folder.join("one.mp3")]);
    }
}
