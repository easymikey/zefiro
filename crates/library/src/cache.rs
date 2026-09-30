use std::{path::Path, sync::Arc};

use bincode::config::Config;
use kernel::{LibrarySubject, Tagging, Track};

use crate::{dirs::LibraryDirs, error::Error, record::TrackRecord};

const CACHE_VERSION: u8 = 5;
const CACHE_LIMIT: usize = 64 << 20;

fn bincode_config() -> impl Config {
    bincode::config::standard().with_limit::<CACHE_LIMIT>()
}

pub(crate) fn encode(tracks: &[&Track], path: &Path) -> Result<Vec<u8>, Error> {
    let records: Vec<TrackRecord> =
        tracks.iter().copied().map(TrackRecord::from).collect();
    let mut bytes = vec![CACHE_VERSION];
    bytes.extend(
        bincode::serde::encode_to_vec(&records, bincode_config()).map_err(
            |source| Error::Encode {
                path: path.to_path_buf(),
                source,
            },
        )?,
    );
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMiss {
    Missing,
    Outdated,
    Corrupt,
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Arc<Track>>, CacheMiss> {
    let (version, rest) = bytes.split_first().ok_or(CacheMiss::Corrupt)?;
    if *version != CACHE_VERSION {
        return Err(CacheMiss::Outdated);
    }
    let (records, _): (Vec<TrackRecord>, usize) =
        bincode::serde::decode_from_slice(rest, bincode_config())
            .map_err(|_| CacheMiss::Corrupt)?;
    records
        .into_iter()
        .map(|record| record.into_track().map(Arc::new).ok_or(CacheMiss::Corrupt))
        .collect()
}

fn absent_or_corrupt(error: &std::io::Error) -> CacheMiss {
    if error.kind() == std::io::ErrorKind::NotFound {
        CacheMiss::Missing
    } else {
        CacheMiss::Corrupt
    }
}

pub(crate) fn load(
    dirs: &LibraryDirs,
    music_dir: &Path,
) -> Result<Vec<Arc<Track>>, CacheMiss> {
    let cache_path = dirs.cache_dir.join("library.bin");
    let dir_path = dirs.cache_dir.join("library.dir");
    let saved_dir = std::fs::read_to_string(&dir_path)
        .map_err(|error| absent_or_corrupt(&error))?;
    if saved_dir.trim() != music_dir.to_string_lossy() {
        return Err(CacheMiss::Missing);
    }
    let bytes =
        std::fs::read(&cache_path).map_err(|error| absent_or_corrupt(&error))?;
    decode(&bytes)
}

pub(crate) fn save(
    dirs: &LibraryDirs,
    music_dir: &Path,
    tracks: &[Arc<Track>],
) -> Result<(), Error> {
    let read: Vec<&Track> = tracks
        .iter()
        .map(Arc::as_ref)
        .filter(|track| track.tagging() == Tagging::Read)
        .collect();
    if read.is_empty() {
        return Ok(());
    }
    let cache_path = dirs.cache_dir.join("library.bin");
    let dir_path = dirs.cache_dir.join("library.dir");
    crate::files::create_parent_dir(&cache_path)
        .map_err(Error::write(LibrarySubject::Cache, &dirs.cache_dir))?;
    crate::files::write_atomic(&cache_path, &encode(&read, &cache_path)?)
        .map_err(Error::write(LibrarySubject::Cache, &cache_path))?;
    crate::files::write_atomic(&dir_path, music_dir.to_string_lossy().as_bytes())
        .map_err(Error::write(LibrarySubject::Cache, &dir_path))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use kernel::{Tags, Track};
    use rstest::rstest;

    use crate::{
        cache::{self, CacheMiss},
        dirs::LibraryDirs,
        test_support,
    };

    fn track(path: &str, title: Option<&str>) -> Arc<Track> {
        Arc::new(test_support::track(
            path,
            Tags {
                title: title.map(str::to_string),
                ..Tags::default()
            },
        ))
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
        let dirs = LibraryDirs::under(directory.path());
        let music_dir = directory.path().join("music");
        let tracks = vec![
            track("/music/one.flac", Some("Moon River")),
            track("/music/two.flac", Some("Clair de Lune")),
        ];

        cache::save(&dirs, &music_dir, &tracks).unwrap();
        let loaded = cache::load(&dirs, &music_dir);

        insta::assert_debug_snapshot!(loaded);
    }

    #[test]
    fn save_skips_tracks_that_were_only_listed_not_read() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());
        let music_dir = directory.path().join("music");
        let listed = vec![Arc::new(Track::listed(Path::new("/music/unreadable.flac")))];

        cache::save(&dirs, &music_dir, &listed).unwrap();
        let loaded = cache::load(&dirs, &music_dir);

        assert_eq!(loaded, Err(CacheMiss::Missing));
    }

    fn write_version(dirs: &LibraryDirs, version: u8) {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        let music_dir = Path::new("/music");
        cache::save(dirs, music_dir, &tracks).unwrap();
        let cache_path = dirs.cache_dir.join("library.bin");
        let mut bytes = std::fs::read(&cache_path).unwrap();
        if let Some(first) = bytes.first_mut() {
            *first = version;
        }
        std::fs::write(&cache_path, bytes).unwrap();
    }

    fn write_bytes(dirs: &LibraryDirs, bytes: Vec<u8>) {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        let music_dir = Path::new("/music");
        cache::save(dirs, music_dir, &tracks).unwrap();
        let cache_path = dirs.cache_dir.join("library.bin");
        std::fs::write(&cache_path, bytes).unwrap();
    }

    #[rstest]
    #[case::never_saved(|_library_paths: &LibraryDirs| {}, CacheMiss::Missing)]
    #[case::other_root(|dirs: &LibraryDirs| {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        cache::save(dirs, Path::new("/other"), &tracks).unwrap();
    }, CacheMiss::Missing)]
    #[case::wrong_version(|dirs: &LibraryDirs| write_version(dirs, 1), CacheMiss::Outdated)]
    #[case::garbage_bytes(|dirs: &LibraryDirs| {
        write_bytes(dirs, vec![cache::CACHE_VERSION, 0xDE, 0xAD, 0xBE, 0xEF]);
    }, CacheMiss::Corrupt)]
    #[case::empty_file(|dirs: &LibraryDirs| {
        write_bytes(dirs, Vec::new());
    }, CacheMiss::Corrupt)]
    fn a_cache_read_names_its_miss(
        #[case] setup: fn(&LibraryDirs),
        #[case] expected: CacheMiss,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());
        setup(&dirs);

        let loaded = cache::load(&dirs, Path::new("/music"));

        assert_eq!(loaded, Err(expected));
    }

    #[test]
    fn a_saved_empty_library_loads_back_empty() {
        let tracks: Vec<Arc<Track>> = Vec::new();
        let bytes = encoded(&tracks);
        assert_eq!(cache::decode(&bytes), Ok(tracks));
    }

    #[test]
    fn an_encoded_cache_decodes_to_the_same_entries() {
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
    fn a_saved_track_keeps_its_duration_and_display_after_loading() {
        let tracks = vec![Arc::new(test_support::track_lasting(
            "/music/one.flac",
            Duration::from_secs(259),
            Tags {
                title: Some("Moon River".to_string()),
                ..Tags::default()
            },
        ))];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes);
        assert_eq!(decoded, Ok(tracks));
        insta::assert_debug_snapshot!(decoded);
    }
}
