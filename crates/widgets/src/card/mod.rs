mod chips;
pub(crate) mod compact;
pub(crate) mod headings;
mod meters;
pub mod metrics;

use std::{sync::Arc, time::Duration};

use headings::CardStyle;
use kernel::domain::{
    appearance::{AppearanceSettings, CoverBrackets},
    percent::Percent,
    player::Player,
    playlist::{PlayOrder, RepeatMode},
    speed::Speed,
    time::Moment,
    track::Track,
    transport::Output,
};
use metrics::{BRACKET_MARGIN, CardMetrics, card_metrics, content_rect};
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
        corner_brackets::CornerBracketsWidget,
        inset::Inset,
    },
    repaint::{Presence, next_clock_second},
    spectrum::Spectrum,
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CardView<'a> {
    pub(crate) player: &'a Player,
    pub(crate) speed: Speed,
    pub(crate) volume: Percent,
    pub(crate) spectrum: &'a Spectrum,
    pub(crate) repeat: RepeatMode,
    pub(crate) play_order: &'a PlayOrder,
    pub(crate) displayed_track: Option<&'a Arc<Track>>,
    pub output: &'a Output,
    pub now: Moment,
}

#[derive(Debug, Clone)]
pub enum CardCover {
    Missing,
    Image,
    Text(Arc<[Line<'static>]>),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CardWidget<'a> {
    pub(crate) view: CardView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) cell_aspect: f32,
    pub(crate) cover_sizing: CoverSizing,
    pub(crate) appearance: AppearanceSettings,
    pub(crate) cover_art: &'a CardCover,
}

impl CardView<'_> {
    pub(crate) fn duration(&self) -> Duration {
        self.displayed_track
            .and_then(|track| track.duration())
            .unwrap_or(Duration::ZERO)
    }

    pub(crate) fn position(&self) -> Duration {
        self.player.position_at(self.now)
    }

    pub(crate) fn remaining(&self) -> Duration {
        self.duration().saturating_sub(self.position())
    }
}

impl CardWidget<'_> {
    pub(crate) fn paint(&self, metrics: &CardMetrics, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        let style = CardStyle::from_theme(&self.theme);
        let frame_color: Color = style.border;

        let block = Block::default()
            .borders(Borders::ALL)
            .padding(Inset::card().padding())
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(frame_color))
            .title(" Sifr ")
            .title_style(Style::default().fg(frame_color));
        block.render(area, buffer);

        if !metrics.cover_square.is_empty() {
            self.paint_cover(buffer, (metrics.cover_square, style));
        }
        headings::paint(buffer, self, metrics);
        meters::paint(buffer, self, metrics);
        if let Some(color) = self.bracket_color(style) {
            (&CornerBracketsWidget { color }).render(
                corner_brackets::expand(content_rect(metrics), BRACKET_MARGIN),
                buffer,
            );
        }
    }

    fn bracket_color(&self, style: CardStyle) -> Option<Color> {
        matches!(self.appearance.cover_brackets, CoverBrackets::Shown)
            .then_some(style.accent)
    }

    fn paint_cover(&self, buffer: &mut Buffer, (area, style): (Rect, CardStyle)) {
        match self.cover_art {
            CardCover::Missing => Paragraph::new("No cover")
                .style(Style::default().fg(style.muted_foreground))
                .alignment(Alignment::Center)
                .render(area, buffer),
            CardCover::Image => {}
            CardCover::Text(lines) => {
                let rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                Paragraph::new(lines.to_vec())
                    .render(area.centered_vertically(Constraint::Length(rows)), buffer);
            }
        }
        if let Some(color) = self.bracket_color(style) {
            (&CornerBracketsWidget { color })
                .render(corner_brackets::expand(area, BRACKET_MARGIN), buffer);
        }
    }
}

impl Widget for &CardWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let metrics = card_metrics(area, self.cell_aspect, self.cover_sizing);
        self.paint(&metrics, Canvas { area, buffer });
    }
}

impl<'a> CardView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &crate::scene::Scene<'a>) -> Self {
        Self {
            player: scene.player,
            speed: scene.transport.speed,
            volume: scene.transport.volume,
            spectrum: scene.spectrum,
            repeat: scene.playlist.repeat,
            play_order: &scene.playlist.play_order,
            displayed_track: scene.displayed_track,
            output: &scene.transport.output,
            now: scene.now,
        }
    }
}

#[must_use]
pub fn clock_frame_due(
    player: &Player,
    clock: Presence,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { head, .. } = player else {
        return None;
    };
    (clock == Presence::Shown).then(|| next_clock_second(*head, now))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        appearance::{AppearanceSettings, ProgressTime},
        bounded::Bounded,
        percent::Percent,
        player::{PausedBy, Player, Preload},
        playhead::Playhead,
        playlist::PlayOrder,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track},
        transport::{Output, StreamError},
    };

    use crate::{
        card::{
            CardCover,
            CardView,
            CardWidget,
            clock_frame_due,
            metrics::card_height,
        },
        geometry::{CoverSizing, DEFAULT_CELL_ASPECT},
        primitive::relative_time::format_time,
        repaint::Presence,
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
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
                output: Output::Lost(StreamError::DeviceGone),
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
                displayed_track: self.track.as_ref(),
                output: &self.output,
                now: Moment::default(),
            }
        }
    }

    fn card<'a>(
        view: CardView<'a>,
        theme: &'a Theme,
        appearance: AppearanceSettings,
    ) -> CardWidget<'a> {
        CardWidget {
            view,
            theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
            cell_aspect: DEFAULT_CELL_ASPECT,
            cover_sizing: CoverSizing::default(),
            appearance,
            cover_art: &CardCover::Missing,
        }
    }

    #[test]
    fn the_now_playing_card_shows_title_status_and_meters() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
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
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
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
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
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
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
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
        let appearance = AppearanceSettings {
            progress_time: ProgressTime::Remaining,
            ..AppearanceSettings::default()
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
        let appearance = AppearanceSettings {
            progress_time: ProgressTime::Elapsed,
            ..AppearanceSettings::default()
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
        let mut widget = card(fixture.view(), &theme, AppearanceSettings::default());
        widget.cover_sizing = CoverSizing::Off;
        let text = rendered(60, card_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(!text.contains("No cover"), "got {text:?}");
    }

    fn clock_player(offset: Duration, since: Moment) -> Player {
        Player::Playing {
            track: track("Moon River"),
            head: Playhead::anchored(offset, since, Speed::clamped(1.0)),
            preload: Preload::None,
        }
    }

    #[test]
    fn a_playing_clock_wants_the_next_second() {
        let now = Moment::new(Duration::from_secs(100));
        let player = clock_player(Duration::from_secs(10), now);

        assert_eq!(
            clock_frame_due(&player, Presence::Shown, now),
            Some(Moment::new(
                now.since_epoch() + Duration::from_millis(1_001)
            ))
        );
    }

    #[test]
    fn a_hidden_clock_wants_no_frame() {
        let now = Moment::new(Duration::from_secs(100));
        let player = clock_player(Duration::from_secs(10), now);

        assert_eq!(clock_frame_due(&player, Presence::Hidden, now), None);
    }

    #[test]
    fn a_paused_clock_wants_no_frame() {
        let now = Moment::new(Duration::from_secs(100));
        let player = Player::Paused {
            track: track("Moon River"),
            at: Duration::from_secs(10),
            by: PausedBy::Listener,
        };

        assert_eq!(clock_frame_due(&player, Presence::Shown, now), None);
    }
}
