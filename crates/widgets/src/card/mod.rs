mod chips;
mod compact;
mod headings;
mod meters;
mod metrics;

use std::sync::Arc;

pub(crate) use compact::{CompactCard, height as compact_height};
use config::{Appearance, CoverBrackets};
pub(crate) use headings::{CardStatus, card_status, status_label};
use kernel::{
    domain::{Output, Percent, Player, Speed, Track},
    playlist::{PlayOrder, RepeatMode},
};
pub use metrics::CardMetrics;
pub(crate) use metrics::{CardLayout, content_rect, height, metrics};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    braille::BrailleBuffers,
    geometry::{CellAspect, CoverSizing},
    primitive::{
        canvas::Canvas,
        corner_brackets,
        corner_brackets::CornerRing,
        inset::Inset,
    },
    spectrum::Spectrum,
    theme::ActiveTheme,
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
}

#[derive(Debug, Clone, Copy)]
pub enum CoverArt<'a> {
    Missing,
    Image,
    Text(&'a [Line<'static>]),
}

#[derive(Debug, Clone, Copy)]
pub struct Card<'a> {
    pub view: CardView<'a>,
    pub theme: ActiveTheme<'a>,
    pub cell_aspect: CellAspect,
    pub cover_sizing: CoverSizing,
    pub appearance: Appearance,
    pub cover_art: CoverArt<'a>,
}

#[derive(Debug)]
pub(crate) struct CardContext<'a> {
    pub(crate) view: CardView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) appearance: Appearance,
    pub(crate) metrics: &'a CardMetrics,
    pub(crate) layout: CardLayout,
}

impl Card<'_> {
    pub fn render_in(&self, metrics: &CardMetrics, canvas: Canvas<'_>) {
        render_card(self, metrics, canvas);
    }
}

impl Widget for &Card<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let metrics = metrics(area, self.cell_aspect, self.cover_sizing);
        self.render_in(&metrics, Canvas { area, buffer });
    }
}

fn render_card(card: &Card<'_>, metrics: &CardMetrics, canvas: Canvas<'_>) {
    let Canvas { area, buffer } = canvas;
    let frame_color: Color = card.theme.frame();
    let accent_color: Color = card.theme.accent();

    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Inset::card().padding())
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(frame_color))
        .title(" Sifr ")
        .title_style(Style::default().fg(frame_color));
    block.render(area, buffer);

    let bracket_color = matches!(card.appearance.cover_brackets, CoverBrackets::Shown)
        .then_some(accent_color);
    if !metrics.cover_square.is_empty() {
        paint_cover(
            buffer,
            metrics.cover_square,
            CoverPaint {
                theme: card.theme,
                art: card.cover_art,
                bracket_color,
            },
        );
    }

    let context = CardContext {
        view: card.view,
        theme: card.theme,
        appearance: card.appearance,
        metrics,
        layout: CardLayout::default(),
    };
    headings::paint(buffer, &context);
    let mut spectrum_buffers = BrailleBuffers::default();
    meters::paint(buffer, &context, &mut spectrum_buffers);
    paint_info_brackets(buffer, &context, bracket_color);
}

#[derive(Debug, Clone, Copy)]
struct CoverPaint<'a> {
    theme: ActiveTheme<'a>,
    art: CoverArt<'a>,
    bracket_color: Option<Color>,
}

fn paint_cover(buffer: &mut Buffer, area: Rect, paint: CoverPaint<'_>) {
    match paint.art {
        CoverArt::Missing => Paragraph::new("No cover")
            .style(Style::default().fg(paint.theme.dim()))
            .alignment(Alignment::Center)
            .render(area, buffer),
        CoverArt::Image => {}
        CoverArt::Text(lines) => {
            let rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            Paragraph::new(lines.to_vec())
                .render(area.centered_vertically(Constraint::Length(rows)), buffer);
        }
    }
    if let Some(color) = paint.bracket_color {
        (&CornerRing { color }).render(
            corner_brackets::expand(area, CardLayout::default().bracket_margin),
            buffer,
        );
    }
}

fn paint_info_brackets(
    buffer: &mut Buffer,
    context: &CardContext<'_>,
    color: Option<Color>,
) {
    let Some(color) = color else {
        return;
    };
    (&CornerRing { color }).render(
        corner_brackets::expand(
            content_rect(context.metrics),
            context.layout.bracket_margin,
        ),
        buffer,
    );
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use config::{Appearance, ProgressStyle};
    use kernel::{
        Bounded,
        domain::{
            AudioFormat,
            Output,
            Percent,
            Player,
            Preload,
            Speed,
            Tags,
            Track,
            format_time,
        },
        playlist::PlayOrder,
    };

    use crate::{
        card::{Card, CardView, CoverArt, height},
        geometry::{CellAspect, CoverSizing},
        scene::fixtures::{noir, painted},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        theme::{ActiveTheme, ColorDepth, Theme},
    };

    fn track(title: &str, duration_secs: u64) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("/music/{title}.mp3"))
                .duration(Duration::from_secs(duration_secs))
                .tags(Tags {
                    title: Some(title.to_string()),
                    artist: Some("Test Artist".to_string()),
                    ..Tags::default()
                })
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

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
                    at: Duration::from_secs(30),
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
                    reason: "device removed".to_string(),
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
            cell_aspect: CellAspect::default(),
            cover_sizing: CoverSizing::default(),
            appearance,
            cover_art: CoverArt::Missing,
        }
    }

    #[test]
    fn the_now_playing_card_shows_title_status_and_meters() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River", 245));
        let widget = card(fixture.view(), &theme, Appearance::default());
        insta::assert_snapshot!(painted(&widget, 60, height()));
    }

    #[test]
    fn no_track_shows_a_placeholder_title_and_a_stopped_status() {
        let theme = noir();
        let fixture = Fixture::stopped();
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = painted(&widget, 60, height());
        assert!(text.contains("No track"), "got {text:?}");
        assert!(text.contains("Stopped"), "got {text:?}");
    }

    #[test]
    fn output_lost_reads_no_output_in_the_status_column() {
        let theme = noir();
        let fixture = Fixture::output_lost(track("Moon River", 245));
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = painted(&widget, 60, height());
        assert!(text.contains("No output"), "got {text:?}");
    }

    #[test]
    fn a_narrow_card_drops_the_format_chip_before_the_elapsed_time() {
        let theme = noir();
        let fixture = Fixture::playing(full_format_track());
        let widget = card(fixture.view(), &theme, Appearance::default());
        let text = painted(&widget, 30, height());
        assert!(!text.contains("KBPS"), "got {text:?}");
        assert!(text.contains("00:3"), "got {text:?}");
    }

    #[test]
    fn progress_style_remaining_shows_a_countdown_chip() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River", 245));
        let appearance = Appearance {
            progress_remaining: ProgressStyle::Remaining,
            ..Appearance::default()
        };
        let widget = card(fixture.view(), &theme, appearance);
        let text = painted(&widget, 60, height());
        let remaining = format!("-{}", format_time(Duration::from_secs(245 - 30)));
        assert!(text.contains(&remaining), "got {text:?}");
    }

    #[test]
    fn progress_style_elapsed_shows_a_plain_fill_bar_without_a_countdown_chip() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River", 245));
        let appearance = Appearance {
            progress_remaining: ProgressStyle::Elapsed,
            ..Appearance::default()
        };
        let widget = card(fixture.view(), &theme, appearance);
        let text = painted(&widget, 60, height());
        let remaining = format!("-{}", format_time(Duration::from_secs(245 - 30)));
        assert!(!text.contains(&remaining), "got {text:?}");
    }

    #[test]
    fn cover_off_omits_the_no_cover_placeholder() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River", 245));
        let mut widget = card(fixture.view(), &theme, Appearance::default());
        widget.cover_sizing = CoverSizing::Off;
        let text = painted(&widget, 60, height());
        assert!(!text.contains("No cover"), "got {text:?}");
    }
}
