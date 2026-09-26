use std::io::Write;

use kernel::{HistoryEntry, LibrarySubject, Track};

use crate::{error::LibraryError, paths::LibraryPaths, record::HistoryRecord};

pub(crate) fn append(
    paths: &LibraryPaths,
    track: &Track,
    at: i64,
) -> Result<(), LibraryError> {
    let path = paths.data.join("history.jsonl");
    crate::files::create_parent(&path).map_err(|source| LibraryError::Write {
        subject: LibrarySubject::History,
        path: path.clone(),
        source,
    })?;
    let entry = HistoryEntry {
        path: track.path().to_path_buf(),
        title: track.song_title(),
        artist: track.tags().artist.clone(),
        at,
    };
    let record = HistoryRecord::from(entry);
    let json = serde_json::to_string(&record).map_err(|source| LibraryError::Json {
        subject: LibrarySubject::History,
        path: path.clone(),
        source,
    })?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| LibraryError::Write {
            subject: LibrarySubject::History,
            path: path.clone(),
            source,
        })?;
    writeln!(file, "{json}").map_err(|source| LibraryError::Write {
        subject: LibrarySubject::History,
        path,
        source,
    })
}

pub(crate) struct HistoryRead {
    pub entries: Vec<HistoryEntry>,
    pub skipped_lines: usize,
}

pub(crate) fn read(
    paths: &LibraryPaths,
    limit: usize,
) -> Result<HistoryRead, LibraryError> {
    let path = paths.data.join("history.jsonl");
    let read = crate::files::read_if_present(&path);
    let Some(contents) = read.map_err(|source| LibraryError::Read {
        subject: LibrarySubject::History,
        path: path.clone(),
        source,
    })?
    else {
        return Ok(HistoryRead {
            entries: Vec::new(),
            skipped_lines: 0,
        });
    };
    Ok(parse_history(&contents, limit))
}

fn parse_history(contents: &str, limit: usize) -> HistoryRead {
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
    HistoryRead {
        entries,
        skipped_lines,
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::{AudioFormat, Tags, Track};
    use rstest::rstest;

    use crate::{
        history::{self, parse_history},
        paths,
        record::HistoryRecord,
    };

    const HISTORY_LOG: &str = include_str!("../tests/fixtures/history.jsonl");
    const FIXTURE_LENGTH: Duration = Duration::from_secs(180);

    fn track(path: &str, title: &str, artist: Option<&str>) -> Track {
        let tags = Tags {
            title: Some(title.to_string()),
            artist: artist.map(str::to_string),
            ..Tags::default()
        };
        Track::builder()
            .path(path)
            .duration(FIXTURE_LENGTH)
            .tags(tags)
            .audio_format(AudioFormat::default())
            .build()
    }

    #[test]
    fn history_append_writes_one_json_line() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        let sample_track = track("/music/song.flac", "Song", Some("Artist"));
        history::append(&library_paths, &sample_track, 1_700_000_000).unwrap();

        let contents =
            std::fs::read_to_string(library_paths.data.join("history.jsonl")).unwrap();
        insta::assert_snapshot!(contents);
    }

    #[test]
    fn read_round_trips_appended_entries_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        let first = track("/music/first.flac", "First", Some("Artist A"));
        let second = track("/music/second.flac", "Second", None);
        history::append(&library_paths, &first, 1_000).unwrap();
        history::append(&library_paths, &second, 2_000).unwrap();

        let read = history::read(&library_paths, 10).unwrap();
        insta::assert_debug_snapshot!(read.entries);
    }

    #[test]
    fn read_returns_empty_vec_when_file_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        let read = history::read(&library_paths, 10).unwrap();

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
        let paths: Vec<PathBuf> = parse_history(HISTORY_LOG, limit)
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(
            paths,
            expected.iter().map(PathBuf::from).collect::<Vec<_>>()
        );
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
