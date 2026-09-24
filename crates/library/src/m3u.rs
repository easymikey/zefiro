use std::{borrow::Borrow, path::PathBuf};

use kernel::Track;

pub(crate) fn to_string<Item: Borrow<Track>>(tracks: &[Item]) -> String {
    std::iter::once("#EXTM3U\n".to_string())
        .chain(tracks.iter().map(|track| entry_line(track.borrow())))
        .collect()
}

fn entry_line(track: &Track) -> String {
    let secs = track.duration().map_or(0, |duration| duration.as_secs());
    let label = match (&track.tags().artist, &track.tags().title) {
        (Some(artist), Some(title)) => format!("{artist} - {title}"),
        _ => track.display().to_string(),
    };
    format!("#EXTINF:{secs},{label}\n{}\n", track.path().display())
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

    use crate::m3u;

    fn track(path: &str, secs: f64, artist_title: (&str, &str)) -> Track {
        let (artist, title) = artist_title;
        Track::builder()
            .path(path)
            .duration(Duration::from_secs_f64(secs))
            .tags(kernel::Tags {
                artist: Some(artist.to_string()),
                title: Some(title.to_string()),
                ..kernel::Tags::default()
            })
            .audio_format(kernel::AudioFormat::default())
            .build()
    }

    #[test]
    fn roundtrip_to_string_then_parse() {
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
    fn parse_cases(#[case] name: &str, #[case] text: &str) {
        insta::assert_debug_snapshot!(name, m3u::parse(text));
    }

    #[test]
    fn to_string_falls_back_to_display_title_without_tags() {
        let tracks = vec![
            Track::builder()
                .path("/no/tags.flac")
                .duration(Duration::from_secs(7))
                .tags(kernel::Tags::default())
                .audio_format(kernel::AudioFormat::default())
                .build(),
        ];
        insta::assert_snapshot!(m3u::to_string(&tracks));
    }
}
