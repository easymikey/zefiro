mod chips;
mod compact;
mod headings;
mod meters;
mod metrics;

use std::{sync::Arc, time::Duration};

pub(crate) use compact::{
    CompactCard,
    compact_height,
    progress_bar_width as compact_progress_bar_width,
};
pub(crate) use headings::{CardStatus, card_status, status_label};
use kernel::{
    Moment,
    domain::{
        Output,
        Percent,
        Player,
        Speed,
        Track,
        appearance::{Appearance, CoverBrackets},
    },
    playlist::{PlayOrder, RepeatMode},
};
pub use metrics::CardMetrics;
pub(crate) use metrics::{
    BRACKET_MARGIN,
    SPECTRUM_MAX_DOTS,
    card_height,
    card_metrics,
    content_rect,
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    geometry::CoverSizing,
    primitive::{
        canvas::Canvas,
        corner_brackets,
        corner_brackets::CornerBrackets,
        inset::Inset,
    },
    spectrum::Spectrum,
    theme::{ActiveTheme, Role},
};

#[derive(Debug, Clone, Copy)]
pub struct CardView<'a> {
    pub player: &'a Player,
    pub speed: Speed,
    pub volume: Percent,
    pub spectrum: &'a Spectrum,
    pub repeat: RepeatMode,
    pub play_order: &'a PlayOrder,
    pub queue_length: usize,
    pub displayed_track: Option<&'a Arc<Track>>,
    pub output: &'a Output,
    pub now: Moment,
}

#[derive(Debug, Clone)]
pub enum CoverArt {
    Missing,
    Image,
    Text(Arc<[Line<'static>]>),
}

#[derive(Debug, Clone, Copy)]
pub struct Card<'a> {
    pub view: CardView<'a>,
    pub theme: ActiveTheme<'a>,
    pub cell_aspect: f32,
    pub cover_sizing: CoverSizing,
    pub appearance: Appearance,
    pub cover_art: &'a CoverArt,
}

impl CardView<'_> {
    pub(crate) fn duration(&self) -> Duration {
        self.displayed_track
            .and_then(|track| track.duration())
            .unwrap_or_default()
    }

    pub(crate) fn position(&self) -> Duration {
        self.player.position_at(self.now)
    }

    pub(crate) fn remaining(&self) -> Duration {
        self.duration().saturating_sub(self.position())
    }
}

impl Card<'_> {
    pub fn render_in(&self, metrics: &CardMetrics, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        let frame_color: Color = self.theme.role(Role::Frame);

        let block = Block::default()
            .borders(Borders::ALL)
            .padding(Inset::card().padding())
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(frame_color))
            .title(" Sifr ")
            .title_style(Style::default().fg(frame_color));
        block.render(area, buffer);

        if !metrics.cover_square.is_empty() {
            self.paint_cover(buffer, metrics.cover_square);
        }
        headings::paint(buffer, self, metrics);
        meters::paint(buffer, self, metrics);
        if let Some(color) = self.bracket_color() {
            (&CornerBrackets { color }).render(
                corner_brackets::expand(content_rect(metrics), BRACKET_MARGIN),
                buffer,
            );
        }
    }

    fn bracket_color(&self) -> Option<Color> {
        matches!(self.appearance.cover_brackets, CoverBrackets::Shown)
            .then(|| self.theme.role(Role::Accent))
    }

    fn paint_cover(&self, buffer: &mut Buffer, area: Rect) {
        match self.cover_art {
            CoverArt::Missing => Paragraph::new("No cover")
                .style(Style::default().fg(self.theme.role(Role::Dim)))
                .alignment(Alignment::Center)
                .render(area, buffer),
            CoverArt::Image => {}
            CoverArt::Text(lines) => {
                let rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                Paragraph::new(lines.to_vec())
                    .render(area.centered_vertically(Constraint::Length(rows)), buffer);
            }
        }
        if let Some(color) = self.bracket_color() {
            (&CornerBrackets { color })
                .render(corner_brackets::expand(area, BRACKET_MARGIN), buffer);
        }
    }
}

impl Widget for &Card<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let metrics = card_metrics(area, self.cell_aspect, self.cover_sizing);
        self.render_in(&metrics, Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::{
        Bounded,
        Moment,
        domain::{
            AudioFormat,
            Output,
            Percent,
            Player,
            Playhead,
            Preload,
            Speed,
            StreamError,
            Tags,
            Track,
            appearance::{Appearance, ProgressTime},
            format_time,
        },
        playlist::PlayOrder,
    };

    use crate::{
        card::{Card, CardView, CoverArt, card_height},
        geometry::{CoverSizing, DEFAULT_CELL_ASPECT},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{ActiveTheme, ColorDepth, Theme},
    };

    fn full_format_track() -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path("/music/moon-river.mp3")
                .duration(Duration::from_secs(245))
                .tags(Tags {
                    title: Some("Moon River".to_string()),
                    artist: Some("Audrey Hepburn".to_string()),
                    ..Tags::default()
                })
                .audio_format(AudioFormat {
                    format: Some("mp3".to_string()),
                    bitrate_kbps: Some(320),
                    sample_rate_hz: Some(44_100),
                    ..AudioFormat::default()
                })
                .build(),
        )
    }

    struct Fixture {
        player: Player,
        spectrum: Spectrum,
        output: Output,
        play_order: PlayOrder,
        track: Option<Arc<Track>>,
    }

    impl Fixture {
        fn playing(track: Arc<Track>) -> Self {
            Self {
                player: Player::Playing {
                    track: Arc::clone(&track),
                    head: Playhead::anchored(
                        Duration::from_secs(30),
                        Moment::default(),
                        Speed::default(),
                    ),
                    preload: Preload::None,
                },
                spectrum: [0.5; SPECTRUM_BANDS],
                output: Output::Ready,
                play_order: PlayOrder::default(),
                track: Some(track),
            }
        }

        fn stopped() -> Self {
            Self {
                player: Player::Stopped,
                spectrum: [0.0; SPECTRUM_BANDS],
                output: Output::Ready,
                play_order: PlayOrder::default(),
                track: None,
            }
        }

        fn output_lost(track: Arc<Track>) -> Self {
            Self {
                player: Player::Stopped,
                spectrum: [0.2; SPECTRUM_BANDS],
                output: Output::Lost {
                    kind: StreamError::DeviceGone,
                },
                play_order: PlayOrder::default(),
                track: Some(track),
            }
        }

        fn view(&self) -> CardView<'_> {
            CardView {
                player: &self.player,
                speed: Speed::default(),
                volume: Percent::clamped(70),
                spectrum: &self.spectrum,
                repeat: Default::default(),
                play_order: &self.play_order,
                queue_length: 3,
                displayed_track: self.track.as_ref(),
                output: &self.output,
                now: Moment::default(),
            }
        }
    }

    fn card<'a>(
        view: CardView<'a>,
        theme: &'a Theme,
        appearance: Appearance,
    ) -> Card<'a> {
        Card {
            view,
            theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
            cell_aspect: DEFAULT_CELL_ASPECT,
            cover_sizing: CoverSizing::default(),
            appearance,
            cover_art: &CoverArt::Missing,
        }
    }

    #[test]
    fn the_now_playing_card_shows_title_status_and_meters() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let widget = card(fixture.view(), &theme, Appearance::default());
        insta::assert_snapshot!(
            rendered(60, card_height(), |frame| frame
                .render_widget(&widget, frame.area()))
            .to_string()
        );
    }

    #[test]
    fn no_track_shows_a_placeholder_title_and_a_stopped_status() {
        let theme = noir();
        let fixture = Fixture::stopped();
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("No track"), "got {text:?}");
        assert!(text.contains("Stopped"), "got {text:?}");
    }

    #[test]
    fn output_lost_reads_no_output_in_the_status_column() {
        let theme = noir();
        let fixture = Fixture::output_lost(track("Moon River"));
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("No output"), "got {text:?}");
    }

    #[test]
    fn a_narrow_card_drops_the_format_chip_before_the_elapsed_time() {
        let theme = noir();
        let fixture = Fixture::playing(full_format_track());
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = rendered(30, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(!text.contains("KBPS"), "got {text:?}");
        assert!(text.contains("00:3"), "got {text:?}");
    }

    #[test]
    fn progress_style_remaining_shows_a_countdown_chip() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let appearance = Appearance {
            progress_time: ProgressTime::Remaining,
            ..Appearance::default()
        };
        let widget = card(fixture.view(), &theme, appearance);
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        let remaining = format!("-{}", format_time(Duration::from_secs(245 - 30)));
        assert!(text.contains(&remaining), "got {text:?}");
    }

    #[test]
    fn progress_style_elapsed_shows_a_plain_fill_bar_without_a_countdown_chip() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let appearance = Appearance {
            progress_time: ProgressTime::Elapsed,
            ..Appearance::default()
        };
        let widget = card(fixture.view(), &theme, appearance);
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        let remaining = format!("-{}", format_time(Duration::from_secs(245 - 30)));
        assert!(!text.contains(&remaining), "got {text:?}");
    }

    #[test]
    fn cover_off_omits_the_no_cover_placeholder() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let mut widget = card(fixture.view(), &theme, Appearance::default());
        widget.cover_sizing = CoverSizing::Off;
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(!text.contains("No cover"), "got {text:?}");
    }
}
