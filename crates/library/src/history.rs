use std::io::Write;

use kernel::{HistoryEntry, Track};

use crate::{
    error::{LibraryError, Subject},
    paths::LibraryPaths,
};

pub(crate) fn append(
    paths: &LibraryPaths,
    track: &Track,
    unix_secs: i64,
) -> Result<(), LibraryError> {
    let path = paths.data.join("history.jsonl");
    crate::files::create_parent(&path).map_err(|source| LibraryError::Write {
        subject: Subject::History,
        path: path.clone(),
        source,
    })?;
    let entry = HistoryEntry {
        path: track.path().to_path_buf(),
        title: track.song_title(),
        artist: track.tags().artist.clone(),
        at: unix_secs,
    };
    let json = serde_json::to_string(&entry).map_err(|source| LibraryError::Json {
        subject: Subject::History,
        path: path.clone(),
        source,
    })?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| LibraryError::Write {
            subject: Subject::History,
            path: path.clone(),
            source,
        })?;
    writeln!(file, "{json}").map_err(|source| LibraryError::Write {
        subject: Subject::History,
        path,
        source,
    })
}

pub(crate) fn read(
    paths: &LibraryPaths,
    limit: usize,
) -> Result<Vec<HistoryEntry>, LibraryError> {
    let path = paths.data.join("history.jsonl");
    let read = crate::files::read_if_present(&path);
    let Some(contents) = read.map_err(|source| LibraryError::Read {
        subject: Subject::History,
        path: path.clone(),
        source,
    })?
    else {
        return Ok(Vec::new());
    };
    Ok(parse_history(&contents, limit))
}

fn parse_history(contents: &str, limit: usize) -> Vec<HistoryEntry> {
    contents
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::{AudioFormat, HistoryEntry, Tags, Track};
    use rstest::rstest;

    use crate::{
        history::{self, parse_history},
        paths,
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

        let entries = history::read(&library_paths, 10).unwrap();
        insta::assert_debug_snapshot!(entries);
    }

    #[test]
    fn read_returns_empty_vec_when_file_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        let entries = history::read(&library_paths, 10).unwrap();

        assert!(entries.is_empty());
    }

    fn parses(line: &&str) -> bool {
        serde_json::from_str::<HistoryEntry>(line).is_ok()
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
            .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
            .filter_map(|entry| serde_json::to_string(&entry).ok())
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
        let paths: Vec<PathBuf> = parse_history(HISTORY_LOG, limit)
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(
            paths,
            expected.iter().map(PathBuf::from).collect::<Vec<_>>()
        );
    }
}
