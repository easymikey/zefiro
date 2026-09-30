use kernel::domain::{Track, format_time};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{Modal, ModalBounds, ModalSize, OverlayAreas, PlacedModal},
    primitive::{
        canvas::Canvas,
        format_chips::kilohertz,
        glyphs::{TrackDetailsGlyphs, TruncateGlyphs},
        span::{line, text},
        text::{truncate_from_left, truncate_to_width},
    },
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 28;
const LEADER_COLUMN: usize = 10;

#[derive(Debug)]
pub(crate) struct TrackDetailsOverlay<'a> {
    pub(crate) track: &'a Track,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) avoid: &'a [Rect],
}

impl TrackDetailsOverlay<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(self.modal().areas(screen, self.avoid))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
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
        let glyphs = TrackDetailsGlyphs::default();
        let rows = self.rows();
        let content_width = rows
            .iter()
            .filter(|detail_row| detail_row.label != glyphs.path_label)
            .map(|detail_row| {
                u16::try_from(detail_row.prefix.width() + detail_row.value.width())
                    .unwrap_or(u16::MAX)
            })
            .max()
            .unwrap_or(0)
            .max(MIN_WIDTH);
        Modal {
            title: glyphs.title_word,
            size: ModalSize::Dialog {
                min_width: MIN_WIDTH,
                content_width,
                content_lines: u16::try_from(rows.len()).unwrap_or(u16::MAX),
            },
            hint: Some(line([text(glyphs.hint).fg(self.theme.dim())])),
            border: self.theme.accent(),
            window_background: self.theme.window_background(),
        }
    }

    fn rows(&self) -> Vec<TrackDetailsRow> {
        value_rows(
            self.track,
            DetailsRowParts {
                glyphs: TrackDetailsGlyphs::default(),
                leader_column: LEADER_COLUMN,
            },
        )
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let glyphs = TrackDetailsGlyphs::default();
        self.rows()
            .into_iter()
            .map(|detail_row| {
                let budget = width.saturating_sub(detail_row.prefix.width());
                let value = if detail_row.label == glyphs.path_label {
                    truncate_from_left(&detail_row.value, budget).into_owned()
                } else {
                    truncate_to_width(
                        &detail_row.value,
                        budget,
                        TruncateGlyphs::default(),
                    )
                    .into_owned()
                };
                line([
                    text(detail_row.prefix).fg(self.theme.dim()),
                    text(value).fg(self.theme.text()),
                ])
            })
            .collect()
    }
}

impl Widget for &TrackDetailsOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

struct TrackDetailsRow {
    label: &'static str,
    prefix: String,
    value: String,
}

#[derive(Debug, Clone, Copy)]
struct DetailsRowParts {
    glyphs: TrackDetailsGlyphs,
    leader_column: usize,
}

fn value_rows(track: &Track, context: DetailsRowParts) -> Vec<TrackDetailsRow> {
    let glyphs = context.glyphs;
    let tags = track.tags();
    [
        (
            glyphs.title_label,
            RowPrefix::Leader,
            missing_or_value(tags.title.clone(), glyphs),
        ),
        (
            glyphs.artist_label,
            RowPrefix::Leader,
            missing_or_value(tags.artist.clone(), glyphs),
        ),
        (
            glyphs.album_label,
            RowPrefix::Leader,
            missing_or_value(tags.album.clone(), glyphs),
        ),
        (
            glyphs.year_label,
            RowPrefix::Plain,
            missing_or_value(tags.date.clone(), glyphs),
        ),
        (
            glyphs.track_label,
            RowPrefix::Plain,
            track_number(track, glyphs),
        ),
        (
            glyphs.duration_label,
            RowPrefix::Plain,
            track
                .duration()
                .map_or_else(|| glyphs.missing.to_string(), format_time),
        ),
        (
            glyphs.format_label,
            RowPrefix::Plain,
            format_summary(track, glyphs),
        ),
        (
            glyphs.path_label,
            RowPrefix::Plain,
            track.path().display().to_string(),
        ),
    ]
    .into_iter()
    .map(|(label, shape, value)| TrackDetailsRow {
        label,
        prefix: shape.prefix(label, context),
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
    fn prefix(self, label: &str, context: DetailsRowParts) -> String {
        match self {
            Self::Leader => leader_prefix(label, context),
            Self::Plain => plain_prefix(label, context.glyphs),
        }
    }
}

fn leader_prefix(label: &str, context: DetailsRowParts) -> String {
    let glyphs = context.glyphs;
    let dashes = context
        .leader_column
        .saturating_sub(label.width())
        .saturating_sub(1)
        .max(1);
    format!(
        "{label} {}{}",
        glyphs.leader_dash.to_string().repeat(dashes),
        glyphs.gap
    )
}

fn plain_prefix(label: &str, glyphs: TrackDetailsGlyphs) -> String {
    format!("{label}{}", glyphs.gap)
}

fn track_number(track: &Track, glyphs: TrackDetailsGlyphs) -> String {
    match (track.tags().track, track.tags().track_total) {
        (Some(number), Some(total)) => format!("{number}/{total}"),
        (Some(number), None) => number.to_string(),
        (None, _) => glyphs.missing.to_string(),
    }
}

fn format_summary(track: &Track, glyphs: TrackDetailsGlyphs) -> String {
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
        glyphs.missing.to_string()
    } else {
        parts.join(" · ")
    }
}

fn missing_or_value(tag: Option<String>, glyphs: TrackDetailsGlyphs) -> String {
    tag.unwrap_or_else(|| glyphs.missing.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{AudioFormat, Tags, Track};

    use crate::{
        overlay::track_details::TrackDetailsOverlay,
        scene::fixtures::{noir, painted},
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
        let overlay = TrackDetailsOverlay {
            track: &track,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 24));
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
        let overlay = TrackDetailsOverlay {
            track: &track,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 48, 16));
    }

    #[test]
    fn track_details_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let track = full_track();
        let overlay = TrackDetailsOverlay {
            track: &track,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        let _ = painted(&overlay, 4, 3);
    }
}
