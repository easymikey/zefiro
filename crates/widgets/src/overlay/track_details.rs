use kernel::domain::{Track, format_time};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{
        Modal,
        ModalBounds,
        ModalRowStyle,
        ModalSize,
        OverlayAreas,
        PlacedModal,
    },
    primitive::{
        canvas::Canvas,
        format_chips::kilohertz,
        glyphs,
        span::{line, text},
        text::{truncate, truncate_from_left},
    },
};

const MIN_WIDTH: u16 = 28;
const LEADER_COLUMN: usize = 10;

#[derive(Debug)]
pub(crate) struct TrackDetailsWidget<'a> {
    pub(crate) track: &'a Track,
    pub(crate) style: ModalRowStyle,
    pub(crate) avoid: &'a [Rect],
}

impl TrackDetailsWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(self.modal().areas(screen, self.avoid))
    }

    fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::Dialog(areas) = areas else {
            return;
        };
        let Canvas { area, buffer } = canvas;
        self.modal().paint(
            PlacedModal {
                areas,
                bounds: ModalBounds {
                    area,
                    avoid: self.avoid,
                },
            },
            buffer,
        );
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(areas.body.width)))
            .render(areas.body, buffer);
    }

    fn modal(&self) -> Modal<'static> {
        let rows = self.rows();
        let content_width = rows
            .iter()
            .filter(|detail_row| detail_row.label != glyphs::track_details::PATH_LABEL)
            .map(|detail_row| {
                u16::try_from(detail_row.prefix.width() + detail_row.value.width())
                    .unwrap_or(u16::MAX)
            })
            .max()
            .unwrap_or(0)
            .max(MIN_WIDTH);
        Modal {
            title: glyphs::track_details::TITLE_WORD,
            size: ModalSize::Dialog {
                min_width: MIN_WIDTH,
                content_width,
                content_lines: u16::try_from(rows.len()).unwrap_or(u16::MAX),
            },
            hint: Some(line([
                text(glyphs::track_details::HINT).fg(self.style.muted_foreground)
            ])),
            border: self.style.accent,
            window_background: self.style.background,
        }
    }

    fn rows(&self) -> Vec<TrackDetailsRow> {
        value_rows(self.track)
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        self.rows()
            .into_iter()
            .map(|detail_row| {
                let budget = width.saturating_sub(detail_row.prefix.width());
                let value = if detail_row.label == glyphs::track_details::PATH_LABEL {
                    truncate_from_left(&detail_row.value, budget).into_owned()
                } else {
                    truncate(&detail_row.value, budget).into_owned()
                };
                line([
                    text(detail_row.prefix).fg(self.style.muted_foreground),
                    text(value).fg(self.style.foreground),
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

struct TrackDetailsRow {
    label: &'static str,
    prefix: String,
    value: String,
}

fn value_rows(track: &Track) -> Vec<TrackDetailsRow> {
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
        (
            glyphs::track_details::PATH_LABEL,
            RowPrefix::Plain,
            track.path().display().to_string(),
        ),
    ]
    .into_iter()
    .map(|(label, shape, value)| TrackDetailsRow {
        label,
        prefix: shape.prefix(label),
        value,
    })
    .collect()
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
        (Some(number), Some(total)) => format!("{number}/{total}"),
        (Some(number), None) => number.to_string(),
        (None, _) => glyphs::track_details::MISSING.to_string(),
    }
}

fn format_summary(track: &Track) -> String {
    let audio_format = track.audio_format();
    let mut parts = Vec::new();
    if let Some(format) = &audio_format.format {
        parts.push(format.to_uppercase());
    }
    if let Some(bitrate_kbps) = audio_format.bitrate_kbps {
        parts.push(format!("{bitrate_kbps} kbps"));
    }
    if let Some(sample_rate_hz) = audio_format.sample_rate_hz {
        parts.push(format!("{:.1} kHz", kilohertz(sample_rate_hz)));
    }
    if parts.is_empty() {
        glyphs::track_details::MISSING.to_string()
    } else {
        parts.join(" · ")
    }
}

fn missing_or_value(tag: Option<String>) -> String {
    tag.unwrap_or_else(|| glyphs::track_details::MISSING.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{AudioFormat, Tags, Track};

    use crate::{
        overlay::{modal::ModalRowStyle, track_details::TrackDetailsWidget},
        test_support::{noir, rendered},
        theme::{ActiveTheme, ColorDepth},
    };

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
                bitrate_kbps: Some(320),
                sample_rate_hz: Some(44100),
                ..AudioFormat::default()
            })
            .build()
    }

    #[test]
    fn track_details_overlay_shows_every_row_at_80x24() {
        let theme = noir();
        let track = full_track();
        let overlay = TrackDetailsWidget {
            track: &track,
            style: ModalRowStyle::from_theme(&ActiveTheme::new(
                &theme,
                ColorDepth::TrueColor,
            )),
            avoid: &[],
        };
        insta::assert_snapshot!(
            rendered(80, 24, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn track_details_overlay_truncates_a_long_path_from_the_left_at_48x16() {
        let theme = noir();
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
            style: ModalRowStyle::from_theme(&ActiveTheme::new(
                &theme,
                ColorDepth::TrueColor,
            )),
            avoid: &[],
        };
        insta::assert_snapshot!(
            rendered(48, 16, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn track_details_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let track = full_track();
        let overlay = TrackDetailsWidget {
            track: &track,
            style: ModalRowStyle::from_theme(&ActiveTheme::new(
                &theme,
                ColorDepth::TrueColor,
            )),
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
