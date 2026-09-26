use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{LibrarySubject, Track};

use crate::{error::LibraryError, tags::read_track};

#[must_use]
pub(crate) fn has_audio(path: &Path, decodable: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| decodable.contains(&extension.as_str()))
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Skips {
    pub count: usize,
    pub first_error: Option<LibraryError>,
}

#[must_use]
#[derive(Debug)]
pub(crate) struct ScanReport {
    pub tracks: Vec<Arc<Track>>,
    pub skipped: Skips,
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub paths: Vec<PathBuf>,
    pub skipped: Skips,
}

impl Listing {
    fn keeping(mut self, entry: walkdir::DirEntry, decodable: &[&str]) -> Self {
        if has_audio(entry.path(), decodable) {
            self.paths.push(entry.into_path());
        }
        self
    }

    fn skipping(mut self, error: walkdir::Error, root: &Path) -> Self {
        self.skipped.count += 1;
        if self.skipped.first_error.is_none() {
            self.skipped.first_error = Some(walk_error(error, root));
        }
        self
    }
}

fn walk_error(error: walkdir::Error, root: &Path) -> LibraryError {
    let path = error
        .path()
        .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
    let text = error.to_string();
    let source = error
        .into_io_error()
        .unwrap_or_else(|| std::io::Error::other(text));
    LibraryError::Read {
        subject: LibrarySubject::Scan,
        path,
        source,
    }
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct TagPass {
    pub tracks: Vec<Arc<Track>>,
    pub lost: usize,
}

pub(crate) fn scan_dir(root: &Path, decodable: &[&str]) -> ScanReport {
    let Listing { paths, skipped } = list_dir(root, decodable);
    let TagPass { tracks, lost } = tag_tracks(&paths);
    let unreadable = paths.len().saturating_sub(tracks.len());
    ScanReport {
        tracks,
        skipped: Skips {
            count: skipped.count + unreadable,
            first_error: skipped.first_error.or_else(|| lost_error(root, lost)),
        },
    }
}

fn lost_error(root: &Path, lost: usize) -> Option<LibraryError> {
    (lost > 0).then(|| LibraryError::Read {
        subject: LibrarySubject::Scan,
        path: root.to_path_buf(),
        source: std::io::Error::other("a tag-reading worker died"),
    })
}

pub(crate) fn list_dir(root: &Path, decodable: &[&str]) -> Listing {
    if !root.is_dir() {
        return Listing {
            paths: Vec::new(),
            skipped: Skips {
                count: 1,
                first_error: Some(LibraryError::Read {
                    subject: LibrarySubject::Scan,
                    path: root.to_path_buf(),
                    source: std::io::Error::from(root_error_kind(root)),
                }),
            },
        };
    }
    walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .fold(Listing::default(), |listing, entry| match entry {
            Ok(entry) => listing.keeping(entry, decodable),
            Err(err) => listing.skipping(err, root),
        })
}

fn root_error_kind(root: &Path) -> std::io::ErrorKind {
    match root.exists() {
        true => std::io::ErrorKind::NotADirectory,
        false => std::io::ErrorKind::NotFound,
    }
}

fn read_chunk(chunk: &[PathBuf]) -> Vec<Arc<Track>> {
    chunk
        .iter()
        .filter_map(|path| read_track(path).ok().map(Arc::new))
        .collect()
}

pub(crate) fn tag_tracks(paths: &[PathBuf]) -> TagPass {
    let workers =
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    if workers <= 1 || paths.len() <= 1 {
        return TagPass {
            tracks: read_chunk(paths),
            lost: 0,
        };
    }
    let chunk_size = paths.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk_size)
            .map(|chunk| (chunk.len(), scope.spawn(|| read_chunk(chunk))))
            .collect();
        handles
            .into_iter()
            .fold(TagPass::default(), |mut tagged, (len, handle)| {
                match handle.join() {
                    Ok(read) => tagged.tracks.extend(read),
                    Err(_) => tagged.lost += len,
                }
                tagged
            })
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::Tagging;
    use rstest::{fixture, rstest};

    use crate::{
        error::LibraryError,
        scan::{has_audio, list_dir, scan_dir, tag_tracks},
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
            has_audio(Path::new(name), DECODABLE),
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
            !has_audio(Path::new(name), DECODABLE),
            "{name} cannot be decoded, so it must not count as audio"
        );
    }

    #[rstest]
    #[case("notes.txt")]
    #[case("cover.jpg")]
    #[case("README")]
    fn anything_else_is_passed_over(#[case] name: &str) {
        assert!(
            !has_audio(Path::new(name), DECODABLE),
            "{name} is not audio"
        );
    }

    #[fixture]
    fn tmp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn tmp_filters() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                r"(/private)?/var/folders/[^/]+/[^/]+/T/\.tmp[A-Za-z0-9]+",
                "[tmp]",
            ),
            (r"/tmp/\.tmp[A-Za-z0-9]+", "[tmp]"),
        ]
    }

    #[rstest]
    fn scan_finds_audio_exts_sorted_and_skips_others(tmp_dir: tempfile::TempDir) {
        for name in ["b.mp3", "a.flac", "x.txt", "c.mkv"] {
            std::fs::write(tmp_dir.path().join(name), b"stub").unwrap();
        }
        let report = scan_dir(tmp_dir.path(), DECODABLE);
        assert_eq!(report.skipped.count, 0);
        insta::with_settings!({ filters => tmp_filters() }, {
            insta::assert_debug_snapshot!(report.tracks);
        });
    }

    #[rstest]
    fn listing_names_the_audio_files_before_a_single_tag_is_read(
        tmp_dir: tempfile::TempDir,
    ) {
        for name in ["b.mp3", "a.flac", "notes.txt"] {
            std::fs::write(tmp_dir.path().join(name), b"stub").unwrap();
        }
        let listing = list_dir(tmp_dir.path(), DECODABLE);
        assert_eq!(listing.skipped.count, 0);
        assert!(listing.skipped.first_error.is_none());
        insta::with_settings!({ filters => tmp_filters() }, {
            insta::assert_debug_snapshot!(listing.paths);
        });
    }

    #[rstest]
    fn a_chunk_of_paths_with_unparseable_containers_stays_listed_not_tagged(
        tmp_dir: tempfile::TempDir,
    ) {
        for name in ["a.flac", "b.mp3", "c.mkv"] {
            std::fs::write(tmp_dir.path().join(name), b"stub").unwrap();
        }
        let listing = list_dir(tmp_dir.path(), DECODABLE);
        let chunk = &listing.paths[..2];

        let tracks = tag_tracks(chunk).tracks;

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
        assert!(tag_tracks(&[]).tracks.is_empty());
    }

    #[rstest]
    #[case::a_file_where_a_directory_was_asked_for(Some("one.mp3"))]
    #[case::nothing_at_that_path(None)]
    fn a_root_that_is_not_a_directory_lists_nothing_and_says_why(
        tmp_dir: tempfile::TempDir,
        #[case] name: Option<&str>,
    ) {
        let root = tmp_dir.path().join(name.unwrap_or("gone"));
        if name.is_some() {
            std::fs::write(&root, b"stub").unwrap();
        }

        let listing = list_dir(&root, DECODABLE);

        assert!(listing.paths.is_empty());
        assert_eq!(listing.skipped.count, 1);
        insta::with_settings!({ filters => tmp_filters(), snapshot_suffix => name.unwrap_or("gone") }, {
            insta::assert_debug_snapshot!(listing.skipped.first_error);
        });
    }

    #[rstest]
    #[cfg(unix)]
    fn scan_counts_unreadable_entries_but_still_scans_readable_ones(
        tmp_dir: tempfile::TempDir,
    ) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(tmp_dir.path().join("a.mp3"), b"stub").unwrap();
        let locked = tmp_dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("b.mp3"), b"stub").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let report = scan_dir(tmp_dir.path(), DECODABLE);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .unwrap();

        assert_eq!(report.tracks.len(), 1);
        assert_eq!(report.skipped.count, 1);
        let first_error = report.skipped.first_error.as_ref();
        assert!(
            first_error.is_some_and(|error| matches!(
                error,
                LibraryError::Read { path, .. } if path.as_os_str() != ""
            )),
            "the walk error must keep a non-empty path"
        );
        insta::with_settings!({ filters => tmp_filters() }, {
            insta::assert_debug_snapshot!(report.skipped.first_error);
        });
    }

    #[rstest]
    #[cfg(unix)]
    fn a_track_whose_tags_cannot_be_read_is_counted_not_dropped(
        tmp_dir: tempfile::TempDir,
    ) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(tmp_dir.path().join("readable.mp3"), b"stub").unwrap();
        let sealed = tmp_dir.path().join("sealed.mp3");
        std::fs::write(&sealed, b"stub").unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let report = scan_dir(tmp_dir.path(), DECODABLE);

        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o644))
            .unwrap();

        assert_eq!(report.tracks.len(), 1);
        assert_eq!(report.skipped.count, 1);
    }

    #[rstest]
    fn a_directory_of_readable_tracks_skips_nothing(tmp_dir: tempfile::TempDir) {
        for name in ["a.flac", "b.mp3"] {
            std::fs::write(tmp_dir.path().join(name), b"stub").unwrap();
        }
        let report = scan_dir(tmp_dir.path(), DECODABLE);
        assert_eq!(report.tracks.len(), 2);
        assert_eq!(report.skipped.count, 0);
    }
}
