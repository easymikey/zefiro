use std::{io::Write, path::PathBuf};

use kernel::{
    domain::{history::HistoryEntry, time::Moment, track::TrackSource},
    message::LibrarySubject,
};
use serde::{Deserialize, Serialize};

use crate::{dirs::LibraryDirs, error::Error};

const HISTORY_FILE_NAME: &str = "history.jsonl";

#[derive(Serialize, Deserialize)]
struct HistoryRecord {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    #[serde(rename = "ts")]
    played_at: i64,
}

impl From<HistoryRecord> for HistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        Self {
            track_source: TrackSource::Local(record.path),
            title: record.title,
            artist: record.artist,
            played_at: Moment::new(std::time::Duration::from_secs(
                u64::try_from(record.played_at).unwrap_or(0),
            )),
        }
    }
}

pub(crate) fn append(
    dirs: &LibraryDirs,
    history_entry: &HistoryEntry,
) -> Result<(), Error> {
    let Some(track_path) = history_entry.track_source.local_path() else {
        return Ok(());
    };
    let path = dirs.data_dir.join(HISTORY_FILE_NAME);
    crate::files::create_parent_dir(&path)
        .map_err(Error::io(LibrarySubject::History, &path))?;
    let record = HistoryRecord {
        path: track_path.to_path_buf(),
        title: history_entry.title.clone(),
        artist: history_entry.artist.clone(),
        played_at: i64::try_from(history_entry.played_at.since_epoch().as_secs())
            .unwrap_or(i64::MAX),
    };
    let json = serde_json::to_string(&record)
        .map_err(Error::json(LibrarySubject::History, &path))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(Error::io(LibrarySubject::History, &path))?;
    writeln!(file, "{json}").map_err(Error::io(LibrarySubject::History, &path))
}

pub(crate) fn load(dirs: &LibraryDirs, limit: usize) -> Result<HistoryRead, Error> {
    let path = dirs.data_dir.join(HISTORY_FILE_NAME);
    let read = crate::files::read_if_present(&path);
    let contents = read.map_err(Error::io(LibrarySubject::History, &path))?;
    let (entries, skipped) =
        contents.map_or_else(|| (Vec::new(), None), |text| parse_history(&text, limit));
    Ok(HistoryRead {
        entries,
        skipped: skipped.map(Error::json(LibrarySubject::History, &path)),
    })
}

pub(crate) struct HistoryRead {
    pub(crate) entries: Vec<HistoryEntry>,
    pub(crate) skipped: Option<Error>,
}

fn parse_history(
    contents: &str,
    limit: usize,
) -> (Vec<HistoryEntry>, Option<serde_json::Error>) {
    let read = contents
        .lines()
        .rev()
        .fold(HistoryParse::default(), |read, line| {
            if read.entries.len() == limit {
                read
            } else {
                read.adding(line)
            }
        });
    (read.entries, read.skipped)
}

#[derive(Default)]
struct HistoryParse {
    entries: Vec<HistoryEntry>,
    skipped: Option<serde_json::Error>,
}

impl HistoryParse {
    fn adding(mut self, line: &str) -> Self {
        match serde_json::from_str::<HistoryRecord>(line) {
            Ok(record) => self.entries.push(HistoryEntry::from(record)),
            Err(error) => self.skipped = self.skipped.or(Some(error)),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::domain::{history::HistoryEntry, time::Moment, track::TrackSource};
    use rstest::rstest;

    use crate::{
        dirs::LibraryDirs,
        history::{self, HistoryRecord, parse_history},
    };

    const HISTORY_LOG: &str = include_str!("../tests/fixtures/history.jsonl");

    fn entry(path: &str, title: &str, artist: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            track_source: TrackSource::Local(PathBuf::from(path)),
            title: title.to_string(),
            artist: artist.map(str::to_string),
            played_at: Moment::default(),
        }
    }

    #[test]
    fn append_writes_one_json_line() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };

        let sample_entry = HistoryEntry {
            played_at: Moment::new(std::time::Duration::from_secs(1_700_000_000)),
            ..entry("/music/song.flac", "Song", Some("Artist"))
        };
        history::append(&dirs, &sample_entry).unwrap();

        let contents =
            std::fs::read_to_string(dirs.data_dir.join(history::HISTORY_FILE_NAME))
                .unwrap();
        insta::assert_snapshot!(contents);
    }

    fn parses(line: &&str) -> bool {
        serde_json::from_str::<HistoryRecord>(line).is_ok()
    }

    #[test]
    fn every_readable_fixture_line_serializes_back_to_the_same_bytes() {
        let written: Vec<String> = HISTORY_LOG
            .lines()
            .filter(parses)
            .map(str::to_owned)
            .collect();

        let round_tripped: Vec<String> = HISTORY_LOG
            .lines()
            .filter_map(|line| serde_json::from_str::<HistoryRecord>(line).ok())
            .filter_map(|record| serde_json::to_string(&record).ok())
            .collect();

        assert_eq!(written.len(), 3);
        assert_eq!(round_tripped, written);
    }

    #[test]
    fn parse_history_skips_a_corrupt_line_and_reverses_the_rest() {
        let (entries, skipped) = parse_history(HISTORY_LOG, 10);
        assert_eq!(
            skipped.map(|error| error.to_string()).as_deref(),
            Some("key must be a string at line 1 column 2")
        );
        insta::assert_debug_snapshot!(entries);
    }

    #[rstest]
    #[case::one(1, &["/music/third.flac"])]
    #[case::two_stop_before_the_broken_line(
        2,
        &["/music/third.flac", "/music/also-good.flac"]
    )]
    fn parse_history_caps_at_limit(#[case] limit: usize, #[case] expected: &[&str]) {
        let (entries, skipped) = parse_history(HISTORY_LOG, limit);
        let track_sources: Vec<TrackSource> = entries
            .into_iter()
            .map(|entry| entry.track_source)
            .collect();
        assert_eq!(
            track_sources,
            expected
                .iter()
                .map(|path| TrackSource::Local(PathBuf::from(path)))
                .collect::<Vec<_>>()
        );
        assert!(skipped.is_none(), "{skipped:?}");
    }
}
