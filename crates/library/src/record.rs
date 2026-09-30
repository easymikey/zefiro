use std::{path::PathBuf, time::Duration};

use kernel::{AudioFormat, HistoryEntry, Tagging, Tags, Track, UnixSeconds};
use serde::{Deserialize, Serialize};

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
    bitrate_kbps: Option<u32>,
    sample_rate_hz: Option<u32>,
    bits_per_sample: Option<u8>,
    channels: Option<u8>,
    replay_gain: Option<f32>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Tagging")]
enum TaggingRecord {
    Listed,
    Read,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TrackRecord {
    path: PathBuf,
    duration: Option<Duration>,
    #[serde(with = "TagsRecord")]
    tags: Tags,
    #[serde(with = "AudioFormatRecord")]
    audio_format: AudioFormat,
    display: Box<str>,
    #[serde(with = "TaggingRecord")]
    tagging: Tagging,
}

impl From<&Track> for TrackRecord {
    fn from(track: &Track) -> Self {
        Self {
            path: track.path().to_path_buf(),
            duration: track.duration(),
            tags: track.tags().clone(),
            audio_format: track.audio_format().clone(),
            display: Box::from(track.display()),
            tagging: track.tagging(),
        }
    }
}

impl TrackRecord {
    pub(crate) fn into_track(self) -> Option<Track> {
        match (self.tagging, self.duration) {
            (Tagging::Listed, _) => Some(Track::listed(&self.path)),
            (Tagging::Read, Some(duration)) => Some(
                Track::builder()
                    .path(self.path)
                    .duration(duration)
                    .tags(self.tags)
                    .audio_format(self.audio_format)
                    .build(),
            ),
            (Tagging::Read, None) => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct HistoryRecord {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    #[serde(rename = "ts")]
    at: i64,
}

impl From<HistoryEntry> for HistoryRecord {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            path: entry.path,
            title: entry.title,
            artist: entry.artist,
            at: entry.at.get(),
        }
    }
}

impl From<HistoryRecord> for HistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        Self {
            path: record.path,
            title: record.title,
            artist: record.artist,
            at: UnixSeconds::new(record.at),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::{Tags, Track};
    use rstest::rstest;

    use crate::{record::TrackRecord, test_support};

    fn read_track() -> Track {
        test_support::track(
            "/music/one.flac",
            Tags {
                title: Some("Moon River".to_string()),
                ..Tags::default()
            },
        )
    }

    fn listed_track() -> Track {
        Track::listed(Path::new("/music/one.flac"))
    }

    fn read_without_duration_record() -> TrackRecord {
        let mut record = TrackRecord::from(&read_track());
        record.duration = None;
        record
    }

    #[rstest]
    #[case::read(TrackRecord::from(&read_track()), Some(read_track()))]
    #[case::listed(TrackRecord::from(&listed_track()), Some(listed_track()))]
    #[case::read_without_duration(read_without_duration_record(), None)]
    fn a_record_turns_back_into_its_track(
        #[case] record: TrackRecord,
        #[case] expected: Option<Track>,
    ) {
        assert_eq!(record.into_track(), expected);
    }
}
