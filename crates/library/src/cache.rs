use std::{path::Path, sync::Arc};

use bincode::config::Config;
use kernel::{LibrarySubject, Tagging, Track};

use crate::{error::LibraryError, paths::LibraryPaths, record::TrackRecord};

const CACHE_VERSION: u8 = 5;
const CACHE_LIMIT: usize = 64 << 20;

fn config() -> impl Config {
    bincode::config::standard().with_limit::<CACHE_LIMIT>()
}

pub(crate) fn encode(tracks: &[&Track], path: &Path) -> Result<Vec<u8>, LibraryError> {
    let records: Vec<TrackRecord> =
        tracks.iter().copied().map(TrackRecord::from).collect();
    let mut buf = vec![CACHE_VERSION];
    buf.extend(bincode::serde::encode_to_vec(&records, config()).map_err(
        |source| LibraryError::Cache {
            path: path.to_path_buf(),
            source,
        },
    )?);
    Ok(buf)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMiss {
    Absent,
    Outdated,
    Corrupt,
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Arc<Track>>, CacheMiss> {
    let (version, rest) = bytes.split_first().ok_or(CacheMiss::Corrupt)?;
    if *version != CACHE_VERSION {
        return Err(CacheMiss::Outdated);
    }
    let (records, _): (Vec<TrackRecord>, usize) =
        bincode::serde::decode_from_slice(rest, config())
            .map_err(|_| CacheMiss::Corrupt)?;
    records
        .into_iter()
        .map(|record| record.into_track().map(Arc::new).ok_or(CacheMiss::Corrupt))
        .collect()
}

fn absent_or_corrupt(error: &std::io::Error) -> CacheMiss {
    if error.kind() == std::io::ErrorKind::NotFound {
        CacheMiss::Absent
    } else {
        CacheMiss::Corrupt
    }
}

pub(crate) fn load(
    paths: &LibraryPaths,
    music_dir: &Path,
) -> Result<Vec<Arc<Track>>, CacheMiss> {
    let cache_path = paths.cache.join("library.bin");
    let dir_path = paths.cache.join("library.dir");
    let saved_dir = std::fs::read_to_string(&dir_path)
        .map_err(|error| absent_or_corrupt(&error))?;
    if saved_dir.trim() != music_dir.to_string_lossy() {
        return Err(CacheMiss::Absent);
    }
    let bytes =
        std::fs::read(&cache_path).map_err(|error| absent_or_corrupt(&error))?;
    decode(&bytes)
}

pub(crate) fn save(
    paths: &LibraryPaths,
    music_dir: &Path,
    tracks: &[Arc<Track>],
) -> Result<(), LibraryError> {
    let read: Vec<&Track> = tracks
        .iter()
        .map(Arc::as_ref)
        .filter(|track| track.tagging() == Tagging::Read)
        .collect();
    if read.is_empty() {
        return Ok(());
    }
    let cache_path = paths.cache.join("library.bin");
    let dir_path = paths.cache.join("library.dir");
    crate::files::create_parent(&cache_path).map_err(|source| LibraryError::Write {
        subject: LibrarySubject::Cache,
        path: paths.cache.clone(),
        source,
    })?;
    crate::files::persist(&cache_path, &encode(&read, &cache_path)?).map_err(
        |source| LibraryError::Write {
            subject: LibrarySubject::Cache,
            path: cache_path.clone(),
            source,
        },
    )?;
    crate::files::persist(&dir_path, music_dir.to_string_lossy().as_bytes()).map_err(
        |source| LibraryError::Write {
            subject: LibrarySubject::Cache,
            path: dir_path.clone(),
            source,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use kernel::Track;
    use rstest::rstest;

    use crate::{
        cache::{self, CacheMiss},
        paths,
    };

    const FIXTURE_LENGTH: Duration = Duration::from_secs(180);

    fn track(path: &str, title: Option<&str>) -> Arc<Track> {
        Track::builder()
            .path(path)
            .duration(FIXTURE_LENGTH)
            .tags(kernel::Tags {
                title: title.map(str::to_string),
                ..kernel::Tags::default()
            })
            .audio_format(kernel::AudioFormat::default())
            .build()
            .into()
    }

    fn encoded(tracks: &[Arc<Track>]) -> Vec<u8> {
        let read: Vec<&Track> = tracks.iter().map(Arc::as_ref).collect();
        cache::encode(&read, Path::new("/data/library.bin")).unwrap()
    }

    #[test]
    fn encoded_bytes_are_stable() {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        insta::assert_debug_snapshot!(encoded(&tracks));
    }

    #[test]
    fn save_then_load_round_trips_for_the_same_music_dir() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        let music_dir = directory.path().join("music");
        let tracks = vec![
            track("/music/one.flac", Some("Moon River")),
            track("/music/two.flac", Some("Clair de Lune")),
        ];

        cache::save(&library_paths, &music_dir, &tracks).unwrap();
        let loaded = cache::load(&library_paths, &music_dir);

        insta::assert_debug_snapshot!(loaded);
    }

    #[test]
    fn save_skips_tracks_that_were_only_listed_not_read() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        let music_dir = directory.path().join("music");
        let listed = vec![Arc::new(Track::listed(Path::new("/music/unreadable.flac")))];

        cache::save(&library_paths, &music_dir, &listed).unwrap();
        let loaded = cache::load(&library_paths, &music_dir);

        assert_eq!(loaded, Err(CacheMiss::Absent));
    }

    fn write_version(library_paths: &paths::LibraryPaths, version: u8) {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        let music_dir = Path::new("/music");
        cache::save(library_paths, music_dir, &tracks).unwrap();
        let cache_path = library_paths.cache.join("library.bin");
        let mut bytes = std::fs::read(&cache_path).unwrap();
        if let Some(first) = bytes.first_mut() {
            *first = version;
        }
        std::fs::write(&cache_path, bytes).unwrap();
    }

    fn write_bytes(library_paths: &paths::LibraryPaths, bytes: Vec<u8>) {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        let music_dir = Path::new("/music");
        cache::save(library_paths, music_dir, &tracks).unwrap();
        let cache_path = library_paths.cache.join("library.bin");
        std::fs::write(&cache_path, bytes).unwrap();
    }

    #[rstest]
    #[case::never_saved(|_library_paths: &paths::LibraryPaths| {}, CacheMiss::Absent)]
    #[case::other_root(|library_paths: &paths::LibraryPaths| {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        cache::save(library_paths, Path::new("/other"), &tracks).unwrap();
    }, CacheMiss::Absent)]
    #[case::wrong_version(|library_paths: &paths::LibraryPaths| write_version(library_paths, 1), CacheMiss::Outdated)]
    #[case::garbage_bytes(|library_paths: &paths::LibraryPaths| {
        write_bytes(library_paths, vec![cache::CACHE_VERSION, 0xDE, 0xAD, 0xBE, 0xEF]);
    }, CacheMiss::Corrupt)]
    #[case::empty_file(|library_paths: &paths::LibraryPaths| {
        write_bytes(library_paths, Vec::new());
    }, CacheMiss::Corrupt)]
    fn a_cache_read_names_its_miss(
        #[case] setup: fn(&paths::LibraryPaths),
        #[case] expected: CacheMiss,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        setup(&library_paths);

        let loaded = cache::load(&library_paths, Path::new("/music"));

        assert_eq!(loaded, Err(expected));
    }

    #[test]
    fn empty_tracks_roundtrip() {
        let tracks: Vec<Arc<Track>> = Vec::new();
        let bytes = encoded(&tracks);
        assert_eq!(cache::decode(&bytes), Ok(tracks));
    }

    #[test]
    fn roundtrip_encode_decode() {
        let tracks = vec![
            track("/music/one.flac", Some("Moon River")),
            track("/music/two.flac", None),
        ];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes);
        assert_eq!(decoded, Ok(tracks));
        insta::assert_debug_snapshot!(decoded);
    }

    #[test]
    fn roundtrip_preserves_duration_and_display() {
        let tracks = vec![Arc::new(
            Track::builder()
                .path("/music/one.flac")
                .duration(Duration::from_secs(259))
                .tags(kernel::Tags {
                    title: Some("Moon River".to_string()),
                    ..kernel::Tags::default()
                })
                .audio_format(kernel::AudioFormat::default())
                .build(),
        )];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes);
        assert_eq!(decoded, Ok(tracks));
        insta::assert_debug_snapshot!(decoded);
    }
}
