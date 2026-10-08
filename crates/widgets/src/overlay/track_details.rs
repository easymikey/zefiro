use std::borrow::Cow;

use kernel::domain::{
    geometry::Cells,
    track::{Track, TrackSource},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
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
        span::{line, text},
        time_text::duration_text,
        truncate::{truncate, truncate_head},
    },
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 28;

#[derive(Debug)]
pub(crate) struct TrackDetailsWidget<'a> {
    rows: &'a [TrackDetailsRow<'a>],
    active_theme: ActiveTheme<'a>,
    avoid: &'a [Rect],
}

impl<'a> TrackDetailsWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        rows: &'a [TrackDetailsRow<'a>],
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            rows,
            active_theme,
            avoid: &[],
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(self.modal().areas(screen, self.avoid))
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::Dialog(areas) = areas else {
            return;
        };
        let buffer = canvas.buffer;
        self.modal().paint(areas, buffer);
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(areas.body.width)))
            .render(areas.body, buffer);
    }

    fn modal(&self) -> Modal<'static> {
        let colors = self.active_theme.colors();
        let content_width = self
            .rows
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
                content_rows: Cells(small_count_u16(self.rows.len())),
            },
            hint: Some(line([
                text(glyphs::track_details::HINT).fg(colors.muted_foreground)
            ])),
            border: colors.accent,
            window_background: colors.window_background,
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'a>> {
        let colors = self.active_theme.colors();
        self.rows
            .iter()
            .map(|detail_row| {
                let budget = width.saturating_sub(detail_row.prefix.width());
                let value = match detail_row.truncation {
                    Truncation::Head => truncate_head(&detail_row.value, budget),
                    Truncation::Tail => truncate(&detail_row.value, budget),
                };
                line([
                    text(detail_row.prefix).fg(colors.muted_foreground),
                    text(value).fg(colors.foreground),
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

#[derive(Debug, Clone, PartialEq)]
pub struct TrackDetailsRow<'a> {
    prefix: &'static str,
    value: Cow<'a, str>,
    truncation: Truncation,
}

impl<'a> TrackDetailsRow<'a> {
    #[must_use]
    pub(crate) fn all(track: &'a Track) -> Vec<Self> {
        let rows = match track.source() {
            TrackSource::Local(path) => vec![TrackDetailsRow {
                prefix: glyphs::track_details::PATH_LABEL,
                value: path.to_string_lossy(),
                truncation: Truncation::Head,
            }],
            TrackSource::Server {
                server_name,
                server_track_id,
            } => vec![
                TrackDetailsRow {
                    prefix: glyphs::track_details::SERVER_LABEL,
                    value: Cow::Borrowed(server_name.as_str()),
                    truncation: Truncation::Tail,
                },
                TrackDetailsRow {
                    prefix: glyphs::track_details::ID_LABEL,
                    value: Cow::Borrowed(server_track_id.as_str()),
                    truncation: Truncation::Tail,
                },
            ],
        };
        tag_rows(track)
            .into_iter()
            .map(|(prefix, value)| TrackDetailsRow {
                prefix,
                value,
                truncation: Truncation::Tail,
            })
            .chain(rows)
            .collect()
    }
}

fn tag_rows(track: &Track) -> [(&'static str, Cow<'_, str>); 7] {
    let tags = track.tags();
    [
        (
            glyphs::track_details::TITLE_LABEL,
            missing_or_value(tags.title.as_deref()),
        ),
        (
            glyphs::track_details::ARTIST_LABEL,
            missing_or_value(tags.artist.as_deref()),
        ),
        (
            glyphs::track_details::ALBUM_LABEL,
            missing_or_value(tags.album.as_deref()),
        ),
        (
            glyphs::track_details::YEAR_LABEL,
            missing_or_value(tags.date.as_deref()),
        ),
        (glyphs::track_details::TRACK_LABEL, track_number(track)),
        (
            glyphs::track_details::DURATION_LABEL,
            track
                .duration()
                .map_or(Cow::Borrowed(glyphs::track_details::MISSING), |duration| {
                    Cow::Owned(duration_text(duration))
                }),
        ),
        (glyphs::track_details::FORMAT_LABEL, format_summary(track)),
    ]
}

fn track_number(track: &Track) -> Cow<'static, str> {
    match (track.tags().track_number, track.tags().track_total) {
        (Some(number), Some(total)) => Cow::Owned(format!(
            "{number}{}{total}",
            glyphs::track_details::TRACK_OF
        )),
        (Some(number), None) => Cow::Owned(number.to_string()),
        (None, _) => Cow::Borrowed(glyphs::track_details::MISSING),
    }
}

fn format_summary(track: &Track) -> Cow<'static, str> {
    Some(format_chip_values(track.audio_format()))
        .filter(|values| !values.is_empty())
        .map_or(Cow::Borrowed(glyphs::track_details::MISSING), |values| {
            Cow::Owned(values.join(glyphs::DOT_SEPARATOR))
        })
}

fn missing_or_value(tag: Option<&str>) -> Cow<'_, str> {
    tag.map_or(Cow::Borrowed(glyphs::track_details::MISSING), Cow::Borrowed)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{
        server::{ServerName, ServerTrackId},
        track::{AudioFormat, Hertz, Kbps, Tags, Track, TrackParts, TrackSource},
    };
    use rstest::{fixture, rstest};

    use crate::{
        overlay::track_details::{TrackDetailsRow, TrackDetailsWidget},
        primitive::glyphs::track_details,
        test_support::{noir, rendered},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[fixture]
    fn theme() -> Theme {
        noir()
    }

    fn full_track() -> Track {
        Track::new(TrackParts {
            path: "/music/tiffanys/moon_river.mp3".into(),
            duration: Duration::from_secs(245),
            tags: Tags {
                title: Some("Moon River".to_string()),
                artist: Some("Audrey Hepburn".to_string()),
                album: Some("Breakfast at Tiffany's".to_string()),
                date: Some("1961".to_string()),
                track_number: Some(3),
                track_total: Some(12),
                ..Tags::default()
            },
            audio_format: AudioFormat {
                format: Some("Mp3".to_string()),
                bitrate: Some(Kbps(320)),
                sample_rate: Some(Hertz(44100)),
                ..AudioFormat::default()
            },
        })
    }

    #[rstest]
    fn track_details_overlay_shows_every_row_at_80x24(theme: Theme) {
        let track = full_track();
        let rows = TrackDetailsRow::all(&track);
        let overlay_widget = TrackDetailsWidget::new(
            &rows,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(80, 24, |frame| frame
                .render_widget(&overlay_widget, frame.area()))
            .to_string()
        );
    }

    #[rstest]
    fn track_details_overlay_truncates_a_long_path_from_the_left_at_48x16(
        theme: Theme,
    ) {
        let track = Track::new(TrackParts {
            path: "/Users/listener/Music/Library/Soundtracks/Breakfast_at_Tiffanys/moon_river.mp3".into(),
            duration: Duration::ZERO,
            tags: Tags {
                title: Some("Moon River".to_string()),
                ..Tags::default()
            },
            audio_format: AudioFormat::default(),
        });
        let rows = TrackDetailsRow::all(&track);
        let overlay_widget = TrackDetailsWidget::new(
            &rows,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(48, 16, |frame| frame
                .render_widget(&overlay_widget, frame.area()))
            .to_string()
        );
    }

    #[rstest]
    fn track_details_overlay_does_not_panic_on_a_tiny_terminal(theme: Theme) {
        let track = full_track();
        let rows = TrackDetailsRow::all(&track);
        let overlay_widget = TrackDetailsWidget::new(
            &rows,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        assert_eq!(
            rendered(4, 3, |frame| frame
                .render_widget(&overlay_widget, frame.area()))
            .buffer()
            .area
            .height,
            3
        );
    }

    #[test]
    fn track_details_show_the_server_and_the_id_of_a_server_track() {
        let track = Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("tr-1"),
        });
        let rows = TrackDetailsRow::all(&track);
        let pairs: Vec<(&str, &str)> = rows
            .iter()
            .map(|row| (row.prefix, row.value.as_ref()))
            .collect();
        assert_eq!(
            pairs[pairs.len() - 2..],
            [
                (track_details::SERVER_LABEL, "home"),
                (track_details::ID_LABEL, "tr-1"),
            ]
        );
        assert!(
            pairs
                .iter()
                .all(|(prefix, _)| *prefix != track_details::PATH_LABEL)
        );
    }
}
