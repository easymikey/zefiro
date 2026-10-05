use kernel::domain::{geometry::Cells, track::Track};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{
        frame::{Modal, ModalSize},
        placement::OverlayAreas,
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        format_chips::format_chip_values,
        glyphs,
        relative_time::format_time,
        span::{line, text},
        text::{truncate, truncate_from_left},
    },
    theme::colors::Colors,
};

const MIN_WIDTH: u16 = 28;
const LEADER_COLUMN: usize = 10;

#[derive(Debug)]
pub(crate) struct TrackDetailsWidget<'a> {
    pub(crate) track: &'a Track,
    pub(crate) colors: Colors<Color>,
    pub(crate) avoid: &'a [Rect],
}

impl TrackDetailsWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(
            self.modal(&value_rows(self.track))
                .areas(screen, self.avoid),
        )
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::Dialog(areas) = areas else {
            return;
        };
        let buffer = canvas.buffer;
        let rows = value_rows(self.track);
        self.modal(&rows).paint(areas, buffer);
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        Paragraph::new(self.lines(rows, usize::from(areas.body.width)))
            .render(areas.body, buffer);
    }

    fn modal(&self, rows: &[TrackDetailsRow]) -> Modal<'static> {
        let content_width = rows
            .iter()
            .filter(|detail_row| detail_row.truncation == Truncation::Tail)
            .map(|detail_row| {
                small_count_u16(detail_row.prefix.width() + detail_row.value.width())
            })
            .max()
            .unwrap_or(0)
            .max(MIN_WIDTH);
        Modal {
            title: glyphs::track_details::TITLE_WORD,
            size: ModalSize::Dialog {
                min_width: Cells(MIN_WIDTH),
                content_width: Cells(content_width),
                content_lines: Cells(small_count_u16(rows.len())),
            },
            hint: Some(line([
                text(glyphs::track_details::HINT).fg(self.colors.muted_foreground)
            ])),
            border: self.colors.accent,
            window_background: self.colors.window_background,
        }
    }

    fn lines(&self, rows: Vec<TrackDetailsRow>, width: usize) -> Vec<Line<'static>> {
        rows.into_iter()
            .map(|detail_row| {
                let budget = width.saturating_sub(detail_row.prefix.width());
                let value = match detail_row.truncation {
                    Truncation::Head => truncate_from_left(&detail_row.value, budget),
                    Truncation::Tail => truncate(&detail_row.value, budget),
                }
                .into_owned();
                line([
                    text(detail_row.prefix).fg(self.colors.muted_foreground),
                    text(value).fg(self.colors.text),
                ])
            })
            .collect()
    }
}

impl Widget for &TrackDetailsWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truncation {
    Head,
    Tail,
}

struct TrackDetailsRow {
    prefix: String,
    value: String,
    truncation: Truncation,
}

fn value_rows(track: &Track) -> Vec<TrackDetailsRow> {
    let path = TrackDetailsRow {
        prefix: RowPrefix::Plain.prefix(glyphs::track_details::PATH_LABEL),
        value: track.path().display().to_string(),
        truncation: Truncation::Head,
    };
    tag_rows(track)
        .into_iter()
        .map(|(label, shape, value)| TrackDetailsRow {
            prefix: shape.prefix(label),
            value,
            truncation: Truncation::Tail,
        })
        .chain(std::iter::once(path))
        .collect()
}

fn tag_rows(track: &Track) -> [(&'static str, RowPrefix, String); 7] {
    let tags = track.tags();
    [
        (
            glyphs::track_details::TITLE_LABEL,
            RowPrefix::Leader,
            missing_or_value(tags.title.clone()),
        ),
        (
            glyphs::track_details::ARTIST_LABEL,
            RowPrefix::Leader,
            missing_or_value(tags.artist.clone()),
        ),
        (
            glyphs::track_details::ALBUM_LABEL,
            RowPrefix::Leader,
            missing_or_value(tags.album.clone()),
        ),
        (
            glyphs::track_details::YEAR_LABEL,
            RowPrefix::Plain,
            missing_or_value(tags.date.clone()),
        ),
        (
            glyphs::track_details::TRACK_LABEL,
            RowPrefix::Plain,
            track_number(track),
        ),
        (
            glyphs::track_details::DURATION_LABEL,
            RowPrefix::Plain,
            track.duration().map_or_else(
                || glyphs::track_details::MISSING.to_string(),
                format_time,
            ),
        ),
        (
            glyphs::track_details::FORMAT_LABEL,
            RowPrefix::Plain,
            format_summary(track),
        ),
    ]
}

#[derive(Debug, Clone, Copy)]
enum RowPrefix {
    Leader,
    Plain,
}

impl RowPrefix {
    fn prefix(self, label: &str) -> String {
        match self {
            Self::Leader => leader_prefix(label),
            Self::Plain => plain_prefix(label),
        }
    }
}

fn leader_prefix(label: &str) -> String {
    let dashes = LEADER_COLUMN
        .saturating_sub(label.width())
        .saturating_sub(1)
        .max(1);
    format!(
        "{label} {}{}",
        glyphs::track_details::LEADER_DASH
            .to_string()
            .repeat(dashes),
        glyphs::track_details::GAP
    )
}

fn plain_prefix(label: &str) -> String {
    format!("{label}{}", glyphs::track_details::GAP)
}

fn track_number(track: &Track) -> String {
    match (track.tags().track, track.tags().track_total) {
        (Some(number), Some(total)) => {
            format!("{number}{}{total}", glyphs::track_details::TRACK_OF)
        }
        (Some(number), None) => number.to_string(),
        (None, _) => glyphs::track_details::MISSING.to_string(),
    }
}

fn format_summary(track: &Track) -> String {
    Some(format_chip_values(track.audio_format()))
        .filter(|values| !values.is_empty())
        .map_or_else(
            || glyphs::track_details::MISSING.to_string(),
            |values| values.join(glyphs::DOT_SEPARATOR),
        )
}

fn missing_or_value(tag: Option<String>) -> String {
    tag.unwrap_or_else(|| glyphs::track_details::MISSING.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::track::{AudioFormat, Hertz, Kbps, Tags, Track};
    use ratatui::style::Color;
    use rstest::{fixture, rstest};

    use crate::{
        overlay::track_details::TrackDetailsWidget,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, colors::Colors, rgb::ColorDepth},
    };

    #[fixture]
    fn colors() -> Colors<Color> {
        ActiveTheme::new(&noir(), ColorDepth::TrueColor).colors()
    }

    fn full_track() -> Track {
        Track::builder()
            .path("/music/tiffanys/moon_river.mp3")
            .duration(Duration::from_secs(245))
            .tags(Tags {
                title: Some("Moon River".to_string()),
                artist: Some("Audrey Hepburn".to_string()),
                album: Some("Breakfast at Tiffany's".to_string()),
                date: Some("1961".to_string()),
                track: Some(3),
                track_total: Some(12),
                ..Tags::default()
            })
            .audio_format(AudioFormat {
                format: Some("Mp3".to_string()),
                bitrate: Some(Kbps(320)),
                sample_rate: Some(Hertz(44100)),
                ..AudioFormat::default()
            })
            .build()
    }

    #[rstest]
    fn track_details_overlay_shows_every_row_at_80x24(colors: Colors<Color>) {
        let track = full_track();
        let overlay = TrackDetailsWidget {
            track: &track,
            colors,
            avoid: &[],
        };
        insta::assert_snapshot!(
            rendered(80, 24, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[rstest]
    fn track_details_overlay_truncates_a_long_path_from_the_left_at_48x16(
        colors: Colors<Color>,
    ) {
        let track = Track::builder()
            .path(
                "/Users/listener/Music/Library/Soundtracks/Breakfast_at_Tiffanys/moon_river.mp3",
            )
            .duration(Duration::ZERO)
            .tags(Tags {
                title: Some("Moon River".to_string()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build();
        let overlay = TrackDetailsWidget {
            track: &track,
            colors,
            avoid: &[],
        };
        insta::assert_snapshot!(
            rendered(48, 16, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[rstest]
    fn track_details_overlay_does_not_panic_on_a_tiny_terminal(colors: Colors<Color>) {
        let track = full_track();
        let overlay = TrackDetailsWidget {
            track: &track,
            colors,
            avoid: &[],
        };
        assert_eq!(
            rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .area
                .height,
            3
        );
    }
}
