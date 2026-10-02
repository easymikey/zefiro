use std::{io::Write, path::PathBuf};

use kernel::{HistoryEntry, LibrarySubject, Moment};
use serde::{Deserialize, Serialize};

use crate::{dirs::LibraryDirs, error::Error};

#[derive(Serialize, Deserialize)]
struct HistoryRecord {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    #[serde(rename = "ts")]
    at: i64,
}

impl From<HistoryRecord> for HistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        Self {
            path: record.path,
            title: record.title,
            artist: record.artist,
            at: Moment::new(std::time::Duration::from_secs(
                u64::try_from(record.at).unwrap_or(0),
            )),
        }
    }
}

pub(crate) fn append(dirs: &LibraryDirs, played: &HistoryEntry) -> Result<(), Error> {
    let path = dirs.data_dir.join("history.jsonl");
    crate::files::create_parent_dir(&path)
        .map_err(Error::io(LibrarySubject::History, &path))?;
    let record = HistoryRecord {
        path: played.path.clone(),
        title: played.title.clone(),
        artist: played.artist.clone(),
        at: i64::try_from(played.at.since_epoch().as_secs()).unwrap_or(i64::MAX),
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

pub(crate) fn load(
    dirs: &LibraryDirs,
    limit: usize,
) -> Result<Vec<HistoryEntry>, Error> {
    let path = dirs.data_dir.join("history.jsonl");
    let read = crate::files::read_if_present(&path);
    let contents = read.map_err(Error::io(LibrarySubject::History, &path))?;
    Ok(contents.map_or_else(Vec::new, |text| parse_history(&text, limit)))
}

fn parse_history(contents: &str, limit: usize) -> Vec<HistoryEntry> {
    contents
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<HistoryRecord>(line).ok())
        .map(HistoryEntry::from)
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{HistoryEntry, Moment};
    use rstest::rstest;

    use crate::{
        dirs::LibraryDirs,
        history::{self, HistoryRecord, parse_history},
    };

    const HISTORY_LOG: &str = include_str!("../tests/fixtures/history.jsonl");

    fn entry(path: &str, title: &str, artist: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            path: PathBuf::from(path),
            title: title.to_string(),
            artist: artist.map(str::to_string),
            at: Moment::default(),
        }
    }

    #[test]
    fn append_writes_one_json_line() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let sample = HistoryEntry {
            at: Moment::new(std::time::Duration::from_secs(1_700_000_000)),
            ..entry("/music/song.flac", "Song", Some("Artist"))
        };
        history::append(&dirs, &sample).unwrap();

        let contents =
            std::fs::read_to_string(dirs.data_dir.join("history.jsonl")).unwrap();
        insta::assert_snapshot!(contents);
    }

    #[test]
    fn load_returns_appended_entries_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let first = HistoryEntry {
            at: Moment::new(std::time::Duration::from_secs(1_000)),
            ..entry("/music/first.flac", "First", Some("Artist A"))
        };
        let second = HistoryEntry {
            at: Moment::new(std::time::Duration::from_secs(2_000)),
            ..entry("/music/second.flac", "Second", None)
        };
        history::append(&dirs, &first).unwrap();
        history::append(&dirs, &second).unwrap();

        let entries = history::load(&dirs, 10).unwrap();
        insta::assert_debug_snapshot!(entries);
    }

    #[test]
    fn a_missing_history_file_loads_empty() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let entries = history::load(&dirs, 10).unwrap();

        assert!(entries.is_empty());
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
        insta::assert_debug_snapshot!(parse_history(HISTORY_LOG, 10));
    }

    #[rstest]
    #[case::one(1, &["/music/third.flac"])]
    #[case::two(2, &["/music/third.flac", "/music/also-good.flac"])]
    #[case::more_than_there_are(9, &["/music/third.flac", "/music/also-good.flac", "/music/good.flac"])]
    fn parse_history_caps_at_limit(#[case] limit: usize, #[case] expected: &[&str]) {
        let dirs: Vec<PathBuf> = parse_history(HISTORY_LOG, limit)
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(dirs, expected.iter().map(PathBuf::from).collect::<Vec<_>>());
    }
}
