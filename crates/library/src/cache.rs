use std::{path::Path, sync::Arc};

use bincode::config::Config;
use kernel::{Tagging, Track};

use crate::{
    error::{LibraryError, Subject},
    paths::LibraryPaths,
};

const CACHE_VERSION: u8 = 5;
const CACHE_LIMIT: usize = 64 << 20;

fn config() -> impl Config {
    bincode::config::standard().with_limit::<CACHE_LIMIT>()
}

pub(crate) fn encode(tracks: &[&Track]) -> Result<Vec<u8>, LibraryError> {
    let mut buf = vec![CACHE_VERSION];
    buf.extend(
        bincode::serde::encode_to_vec(tracks, config()).map_err(LibraryError::Cache)?,
    );
    Ok(buf)
}

#[must_use]
pub(crate) fn decode(bytes: &[u8]) -> Option<Vec<Arc<Track>>> {
    let (version, rest) = bytes.split_first()?;
    if *version != CACHE_VERSION {
        return None;
    }
    bincode::serde::decode_from_slice(rest, config())
        .ok()
        .map(|(tracks, _)| tracks)
}

#[must_use]
pub(crate) fn load(paths: &LibraryPaths, music_dir: &Path) -> Option<Vec<Arc<Track>>> {
    let cache_path = paths.cache.join("library.bin");
    let dir_path = paths.cache.join("library.dir");
    let saved_dir = std::fs::read_to_string(&dir_path).ok()?;
    if saved_dir.trim() != music_dir.to_string_lossy() {
        return None;
    }
    decode(&std::fs::read(&cache_path).ok()?)
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
        subject: Subject::Cache,
        path: paths.cache.clone(),
        source,
    })?;
    crate::files::persist(&cache_path, &encode(&read)?).map_err(|source| {
        LibraryError::Write {
            subject: Subject::Cache,
            path: cache_path.clone(),
            source,
        }
    })?;
    crate::files::persist(&dir_path, music_dir.to_string_lossy().as_bytes()).map_err(
        |source| LibraryError::Write {
            subject: Subject::Cache,
            path: dir_path.clone(),
            source,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::Track;
    use rstest::rstest;

    use crate::{cache, paths};

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
        cache::encode(&read).unwrap()
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
    fn load_returns_none_for_a_music_dir_that_was_never_cached() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        let loaded = cache::load(&library_paths, &directory.path().join("music"));

        assert!(loaded.is_none());
    }

    #[test]
    fn load_returns_none_when_the_music_dir_does_not_match_the_saved_one() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        cache::save(&library_paths, &directory.path().join("music"), &tracks).unwrap();

        let loaded = cache::load(&library_paths, &directory.path().join("other"));

        assert!(loaded.is_none());
    }

    #[test]
    fn save_skips_tracks_that_were_only_listed_not_read() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        let music_dir = directory.path().join("music");
        let listed = vec![Arc::new(Track::listed(std::path::Path::new(
            "/music/unreadable.flac",
        )))];

        cache::save(&library_paths, &music_dir, &listed).unwrap();
        let loaded = cache::load(&library_paths, &music_dir);

        assert!(loaded.is_none());
    }

    #[rstest]
    #[case::wrong_version(vec![9, 1, 2, 3])]
    #[case::garbage_bytes(vec![2u8, 0xDE, 0xAD, 0xBE, 0xEF, 0xFF, 0xFF, 0xFF, 0xFF])]
    #[case::empty_bytes(Vec::new())]
    fn decode_rejects_invalid_bytes(#[case] bytes: Vec<u8>) {
        assert_eq!(cache::decode(&bytes), None);
    }

    #[rstest]
    #[case::bit_flipped_version(|bytes: &mut [u8]| {
        if let Some(first) = bytes.first_mut() {
            *first ^= 0xFF;
        }
    })]
    #[case::other_version(|bytes: &mut [u8]| {
        if let Some(first) = bytes.first_mut() {
            *first = 1;
        }
    })]
    fn decode_rejects_bad_version_byte(#[case] mutate: fn(&mut [u8])) {
        let tracks = vec![track("/music/one.flac", Some("Moon River"))];
        let mut bytes = encoded(&tracks);
        mutate(&mut bytes);
        assert_eq!(cache::decode(&bytes), None);
    }

    #[test]
    fn empty_tracks_roundtrip() {
        let tracks: Vec<Arc<Track>> = Vec::new();
        let bytes = encoded(&tracks);
        assert_eq!(cache::decode(&bytes), Some(tracks));
    }

    #[test]
    fn roundtrip_encode_decode() {
        let tracks = vec![
            track("/music/one.flac", Some("Moon River")),
            track("/music/two.flac", None),
        ];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes);
        assert_eq!(decoded, Some(tracks));
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
        assert_eq!(decoded, Some(tracks));
        insta::assert_debug_snapshot!(decoded);
    }
}
