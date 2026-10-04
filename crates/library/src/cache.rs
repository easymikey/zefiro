use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bincode::config::Config;
use kernel::{
    domain::track::{AudioFormat, Decibels, Hertz, Kbps, Tagging, Tags, Track},
    message::LibrarySubject,
};
use serde::{Deserialize, Serialize};

use crate::{dirs::LibraryDirs, error::Error};

const CACHE_VERSION: u8 = 6;
const CACHE_LIMIT: usize = 64 << 20;

#[derive(Serialize, Deserialize)]
#[serde(remote = "Tags")]
struct TagsRecord {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    date: Option<String>,
    genre: Option<String>,
    track: Option<u32>,
    track_total: Option<u32>,
    disc: Option<u32>,
    composer: Option<String>,
    comment: Option<String>,
    lyrics: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AudioFormat")]
struct AudioFormatRecord {
    format: Option<String>,
    #[serde(with = "kbps_record")]
    bitrate: Option<Kbps>,
    #[serde(with = "hertz_record")]
    sample_rate: Option<Hertz>,
    bits_per_sample: Option<u8>,
    channels: Option<u8>,
    #[serde(with = "decibels_record")]
    replay_gain: Option<Decibels>,
}

mod decibels_record {
    use kernel::domain::track::Decibels;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<G: std::borrow::Borrow<Option<Decibels>>, S: Serializer>(
        gain: &G,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        gain.borrow()
            .map(|decibels| decibels.0)
            .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Decibels>, D::Error> {
        Option::<f32>::deserialize(deserializer).map(|gain| gain.map(Decibels))
    }
}

mod kbps_record {
    use kernel::domain::track::Kbps;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<G: std::borrow::Borrow<Option<Kbps>>, S: Serializer>(
        bitrate: &G,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        bitrate.borrow().map(|kbps| kbps.0).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Kbps>, D::Error> {
        Option::<u32>::deserialize(deserializer).map(|bitrate| bitrate.map(Kbps))
    }
}

mod hertz_record {
    use kernel::domain::track::Hertz;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<G: std::borrow::Borrow<Option<Hertz>>, S: Serializer>(
        sample_rate: &G,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        sample_rate
            .borrow()
            .map(|hertz| hertz.0)
            .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Hertz>, D::Error> {
        Option::<u32>::deserialize(deserializer)
            .map(|sample_rate| sample_rate.map(Hertz))
    }
}

#[derive(Serialize, Deserialize)]
struct TrackRecord {
    path: PathBuf,
    duration: Option<Duration>,
    #[serde(with = "TagsRecord")]
    tags: Tags,
    #[serde(with = "AudioFormatRecord")]
    audio_format: AudioFormat,
}

impl From<&Track> for TrackRecord {
    fn from(track: &Track) -> Self {
        Self {
            path: track.path().to_path_buf(),
            duration: track.duration(),
            tags: track.tags().clone(),
            audio_format: track.audio_format().clone(),
        }
    }
}

impl TrackRecord {
    fn into_track(self) -> Track {
        match self.duration {
            Some(duration) => Track::builder()
                .path(self.path)
                .duration(duration)
                .tags(self.tags)
                .audio_format(self.audio_format)
                .build(),
            None => Track::listed(&self.path),
        }
    }
}

fn bincode_config() -> impl Config {
    bincode::config::standard().with_limit::<CACHE_LIMIT>()
}

fn cache_file(dirs: &LibraryDirs, extension: &str) -> PathBuf {
    dirs.cache_dir.join(format!("library.{extension}"))
}

pub(crate) fn encode(tracks: &[&Track], path: &Path) -> Result<Vec<u8>, Error> {
    let records: Vec<TrackRecord> =
        tracks.iter().copied().map(TrackRecord::from).collect();
    let payload = bincode::serde::encode_to_vec(&records, bincode_config()).map_err(
        |source| Error::Encode {
            path: path.to_path_buf(),
            source,
        },
    )?;
    Ok(std::iter::once(CACHE_VERSION).chain(payload).collect())
}

pub(crate) fn decode(bytes: &[u8], path: &Path) -> Result<Vec<Arc<Track>>, Error> {
    let Some((&CACHE_VERSION, rest)) = bytes.split_first() else {
        return Ok(Vec::new());
    };
    let (records, _) = bincode::serde::decode_from_slice::<Vec<TrackRecord>, _>(
        rest,
        bincode_config(),
    )
    .map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(records
        .into_iter()
        .map(|record| Arc::new(record.into_track()))
        .collect())
}

pub(crate) fn load(
    dirs: &LibraryDirs,
    music_dir: &Path,
) -> Result<Vec<Arc<Track>>, Error> {
    let dir_path = cache_file(dirs, "dir");
    let saved_dir = crate::files::read_if_present(&dir_path)
        .map_err(Error::io(LibrarySubject::Cache, &dir_path))?;
    if saved_dir.is_none_or(|saved| saved.trim() != music_dir.to_string_lossy()) {
        return Ok(Vec::new());
    }
    let cache_path = cache_file(dirs, "bin");
    match std::fs::read(&cache_path) {
        Ok(bytes) => decode(&bytes, &cache_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(Error::io(LibrarySubject::Cache, &cache_path)(error)),
    }
}

pub(crate) fn save(
    dirs: &LibraryDirs,
    music_dir: &Path,
    tracks: &[Arc<Track>],
) -> Result<(), Error> {
    let read: Vec<&Track> = tracks
        .iter()
        .map(Arc::as_ref)
        .filter(|track| matches!(track.tagging(), Tagging::Read(_)))
        .collect();
    if read.is_empty() {
        return Ok(());
    }
    let cache_path = cache_file(dirs, "bin");
    let dir_path = cache_file(dirs, "dir");
    crate::files::create_parent_dir(&cache_path)
        .map_err(Error::io(LibrarySubject::Cache, &dirs.cache_dir))?;
    crate::files::write_atomic(&cache_path, &encode(&read, &cache_path)?)
        .map_err(Error::io(LibrarySubject::Cache, &cache_path))?;
    crate::files::write_atomic(&dir_path, music_dir.to_string_lossy().as_bytes())
        .map_err(Error::io(LibrarySubject::Cache, &dir_path))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use kernel::domain::track::{Tags, Track};
    use rstest::rstest;

    use crate::{cache, dirs::LibraryDirs, test_support};

    fn encoded(tracks: &[Arc<Track>]) -> Vec<u8> {
        let tracks: Vec<&Track> = tracks.iter().map(Arc::as_ref).collect();
        cache::encode(&tracks, Path::new("/data/library.bin")).unwrap()
    }

    #[test]
    fn encoded_bytes_are_stable() {
        let tracks = vec![test_support::titled("/music/one.flac", "Moon River")];
        insta::assert_debug_snapshot!(encoded(&tracks));
    }

    #[test]
    fn save_then_load_round_trips_for_the_same_music_dir() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        let music_dir = directory.path().join("music");
        let tracks = vec![
            test_support::titled("/music/one.flac", "Moon River"),
            test_support::titled("/music/two.flac", "Clair de Lune"),
        ];

        cache::save(&dirs, &music_dir, &tracks).unwrap();
        let loaded = cache::load(&dirs, &music_dir).unwrap();

        insta::assert_debug_snapshot!(loaded);
    }

    #[test]
    fn save_skips_tracks_that_were_only_listed_not_read() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        let music_dir = directory.path().join("music");
        let listed = vec![Arc::new(Track::listed(Path::new("/music/unreadable.flac")))];

        cache::save(&dirs, &music_dir, &listed).unwrap();
        let loaded = cache::load(&dirs, &music_dir).unwrap();

        assert!(loaded.is_empty());
    }

    fn save_then_rewrite(dirs: &LibraryDirs, rewrite: impl FnOnce(&mut Vec<u8>)) {
        let tracks = vec![test_support::titled("/music/one.flac", "Moon River")];
        cache::save(dirs, Path::new("/music"), &tracks).unwrap();
        let cache_path = dirs.cache_dir.join("library.bin");
        let mut bytes = std::fs::read(&cache_path).unwrap();
        rewrite(&mut bytes);
        std::fs::write(&cache_path, bytes).unwrap();
    }

    #[rstest]
    #[case::never_saved(|_dirs: &LibraryDirs| {})]
    #[case::other_music_dir(|dirs: &LibraryDirs| {
        let tracks = vec![test_support::titled("/music/one.flac", "Moon River")];
        cache::save(dirs, Path::new("/other"), &tracks).unwrap();
    })]
    #[case::wrong_version(|dirs: &LibraryDirs| {
        save_then_rewrite(dirs, |bytes| bytes[0] = 1);
    })]
    #[case::empty_file(|dirs: &LibraryDirs| {
        save_then_rewrite(dirs, Vec::clear);
    })]
    fn a_cache_that_cannot_be_used_loads_empty(#[case] setup: fn(&LibraryDirs)) {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        setup(&dirs);

        let loaded = cache::load(&dirs, Path::new("/music")).unwrap();

        assert!(loaded.is_empty());
    }

    #[test]
    fn a_cache_of_garbage_bytes_answers_a_decode_error() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        save_then_rewrite(&dirs, |bytes| {
            *bytes = vec![cache::CACHE_VERSION, 0xDE, 0xAD, 0xBE, 0xEF];
        });

        let loaded = cache::load(&dirs, Path::new("/music"));

        assert!(matches!(loaded, Err(crate::error::Error::Decode { .. })));
    }

    #[test]
    fn a_saved_empty_library_loads_back_empty() {
        let tracks: Vec<Arc<Track>> = Vec::new();
        let bytes = encoded(&tracks);
        assert_eq!(
            cache::decode(&bytes, Path::new("/data/library.bin")).unwrap(),
            tracks
        );
    }

    #[test]
    fn an_encoded_cache_decodes_to_the_same_entries() {
        let tracks = vec![
            test_support::titled("/music/one.flac", "Moon River"),
            Arc::new(test_support::track("/music/two.flac", Tags::default())),
        ];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes, Path::new("/data/library.bin")).unwrap();
        assert_eq!(decoded, tracks);
        insta::assert_debug_snapshot!(decoded);
    }

    #[test]
    fn a_saved_track_keeps_its_duration_after_loading() {
        let tracks = vec![Arc::new(test_support::track_lasting(
            "/music/one.flac",
            Duration::from_secs(259),
            Tags {
                title: Some("Moon River".to_string()),
                ..Tags::default()
            },
        ))];
        let bytes = encoded(&tracks);
        let decoded = cache::decode(&bytes, Path::new("/data/library.bin")).unwrap();
        assert_eq!(decoded, tracks);
        insta::assert_debug_snapshot!(decoded);
    }
}
