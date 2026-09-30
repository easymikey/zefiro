use std::{borrow::Borrow, path::PathBuf};

use kernel::Track;

pub(crate) fn to_string<Item: Borrow<Track>>(tracks: &[Item]) -> String {
    std::iter::once("#EXTM3U\n".to_string())
        .chain(tracks.iter().map(|track| entry_line(track.borrow())))
        .collect()
}

fn entry_line(track: &Track) -> String {
    let seconds = track.duration().map_or(0, |duration| duration.as_secs());
    let label = match (&track.tags().artist, &track.tags().title) {
        (Some(artist), Some(title)) => format!("{artist} - {title}"),
        _ => track.display().to_string(),
    };
    format!("#EXTINF:{seconds},{label}\n{}\n", track.path().display())
}

#[must_use]
pub(crate) fn parse(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.trim_start().starts_with('#'))
        .map(|line| PathBuf::from(line.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::Track;
    use rstest::rstest;

    use crate::{m3u, test_support};

    fn track(path: &str, seconds: f64, artist_title: (&str, &str)) -> Track {
        let (artist, title) = artist_title;
        test_support::track_lasting(
            path,
            Duration::from_secs_f64(seconds),
            kernel::Tags {
                artist: Some(artist.to_string()),
                title: Some(title.to_string()),
                ..kernel::Tags::default()
            },
        )
    }

    #[test]
    fn a_written_playlist_parses_back_to_the_same_tracks() {
        let tracks = vec![
            track("/music/a.flac", 123.0, ("Artist A", "Title A")),
            track("/music/b.mp3", 45.0, ("Artist B", "Title B")),
        ];
        let text = m3u::to_string(&tracks);
        insta::assert_snapshot!(text);
        insta::assert_debug_snapshot!(m3u::parse(&text));
    }

    #[rstest]
    #[case::missing_header(
        "missing_header",
        include_str!("../tests/fixtures/no_header.m3u")
    )]
    #[case::comments_and_blank_lines(
        "comments_and_blank_lines",
        include_str!("../tests/fixtures/comments_and_blanks.m3u")
    )]
    fn parsing_reads_the_tracks_each_line_layout_names(
        #[case] name: &str,
        #[case] text: &str,
    ) {
        insta::assert_debug_snapshot!(name, m3u::parse(text));
    }

    #[test]
    fn to_string_falls_back_to_display_title_without_tags() {
        let tracks = vec![test_support::track_lasting(
            "/no/tags.flac",
            Duration::from_secs(7),
            kernel::Tags::default(),
        )];
        insta::assert_snapshot!(m3u::to_string(&tracks));
    }
}
