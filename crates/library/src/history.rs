use std::io::Write;

use kernel::{HistoryEntry, LibrarySubject, Track, UnixSeconds};

use crate::{dirs::LibraryDirs, error::Error, record::HistoryRecord};

pub(crate) fn append(
    dirs: &LibraryDirs,
    track: &Track,
    at: UnixSeconds,
) -> Result<(), Error> {
    let path = dirs.data_dir.join("history.jsonl");
    crate::files::create_parent_dir(&path)
        .map_err(Error::write(LibrarySubject::History, &path))?;
    let entry = HistoryEntry {
        path: track.path().to_path_buf(),
        title: track.song_title(),
        artist: track.tags().artist.clone(),
        at,
    };
    let record = HistoryRecord::from(entry);
    let json = serde_json::to_string(&record)
        .map_err(Error::json(LibrarySubject::History, &path))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(Error::write(LibrarySubject::History, &path))?;
    writeln!(file, "{json}").map_err(Error::write(LibrarySubject::History, &path))
}

pub(crate) struct LoadedHistory {
    pub entries: Vec<HistoryEntry>,
    pub skipped_lines: usize,
}

pub(crate) fn load(dirs: &LibraryDirs, limit: usize) -> Result<LoadedHistory, Error> {
    let path = dirs.data_dir.join("history.jsonl");
    let read = crate::files::read_if_present(&path);
    let Some(contents) = read.map_err(Error::read(LibrarySubject::History, &path))?
    else {
        return Ok(LoadedHistory {
            entries: Vec::new(),
            skipped_lines: 0,
        });
    };
    Ok(parse_history(&contents, limit))
}

fn parse_history(contents: &str, limit: usize) -> LoadedHistory {
    let mut entries = Vec::new();
    let mut skipped_lines = 0;
    for line in contents.lines().rev() {
        if entries.len() >= limit {
            break;
        }
        match serde_json::from_str::<HistoryRecord>(line) {
            Ok(record) => entries.push(HistoryEntry::from(record)),
            Err(_) => skipped_lines += 1,
        }
    }
    LoadedHistory {
        entries,
        skipped_lines,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{Tags, Track, UnixSeconds};
    use rstest::rstest;

    use crate::{
        dirs::LibraryDirs,
        history::{self, parse_history},
        record::HistoryRecord,
        test_support,
    };

    const HISTORY_LOG: &str = include_str!("../tests/fixtures/history.jsonl");

    fn track(path: &str, title: &str, artist: Option<&str>) -> Track {
        test_support::track(
            path,
            Tags {
                title: Some(title.to_string()),
                artist: artist.map(str::to_string),
                ..Tags::default()
            },
        )
    }

    #[test]
    fn history_append_writes_one_json_line() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let sample_track = track("/music/song.flac", "Song", Some("Artist"));
        history::append(&dirs, &sample_track, UnixSeconds::new(1_700_000_000)).unwrap();

        let contents =
            std::fs::read_to_string(dirs.data_dir.join("history.jsonl")).unwrap();
        insta::assert_snapshot!(contents);
    }

    #[test]
    fn read_round_trips_appended_entries_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let first = track("/music/first.flac", "First", Some("Artist A"));
        let second = track("/music/second.flac", "Second", None);
        history::append(&dirs, &first, UnixSeconds::new(1_000)).unwrap();
        history::append(&dirs, &second, UnixSeconds::new(2_000)).unwrap();

        let read = history::load(&dirs, 10).unwrap();
        insta::assert_debug_snapshot!(read.entries);
    }

    #[test]
    fn read_returns_empty_vec_when_file_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        let read = history::load(&dirs, 10).unwrap();

        assert!(read.entries.is_empty());
        assert_eq!(read.skipped_lines, 0);
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
        insta::assert_debug_snapshot!(parse_history(HISTORY_LOG, 10).entries);
    }

    #[rstest]
    #[case::one(1, &["/music/third.flac"])]
    #[case::two(2, &["/music/third.flac", "/music/also-good.flac"])]
    #[case::more_than_there_are(9, &["/music/third.flac", "/music/also-good.flac", "/music/good.flac"])]
    fn parse_history_caps_at_limit(#[case] limit: usize, #[case] expected: &[&str]) {
        let dirs: Vec<PathBuf> = parse_history(HISTORY_LOG, limit)
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(dirs, expected.iter().map(PathBuf::from).collect::<Vec<_>>());
    }

    #[rstest]
    #[case::the_corrupt_line_is_older_than_the_first_entry_asked_for(1, 0)]
    #[case::the_whole_file_is_visited(9, 1)]
    fn parse_history_counts_the_lines_it_skips(
        #[case] limit: usize,
        #[case] expected_skipped: usize,
    ) {
        let read = parse_history(HISTORY_LOG, limit);
        assert_eq!(read.skipped_lines, expected_skipped);
    }
}
