mod chips;
pub(crate) mod compact;
pub(crate) mod headings;
mod meters;
pub mod metrics;

use std::{sync::Arc, time::Duration};

use headings::CardStatus;
use kernel::domain::{
    appearance::{AppearanceSettings, CoverBrackets},
    geometry::Cells,
    percent::Percent,
    player::Player,
    playlist::{PlayOrder, RepeatMode},
    revision::Revision,
    speed::Speed,
    time::Moment,
    track::Track,
    transport::OutputStatus,
};
use metrics::{BRACKET_MARGIN, CardMetrics, content_rect};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    geometry::{CoverSizing, DEFAULT_CELL_ASPECT},
    pixels::numeric::{small_count_u16, unit_fraction},
    primitive::{
        bar::remaining_label,
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
    pub(crate) repeat_mode: RepeatMode,
    pub(crate) play_order: &'a PlayOrder,
    pub(crate) displayed_track: Option<&'a Arc<Track>>,
    pub(crate) output_status: &'a OutputStatus,
    pub(crate) buffering_revision: Option<Revision>,
    pub(crate) now: Moment,
}

#[derive(Debug, Clone)]
pub enum CardCover {
    Missing,
    Image,
    Text(Arc<[Line<'static>]>),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CardWidget<'a> {
    view: CardView<'a>,
    active_theme: ActiveTheme<'a>,
    cell_aspect: f32,
    cover_sizing: CoverSizing,
    appearance_settings: AppearanceSettings,
    card_cover: &'a CardCover,
    progress_bar_width: Cells,
    remaining_label: &'a str,
}

const NO_TRACK_TITLE: &str = "No track";
const NO_COVER_TEXT: &str = "No cover";
const CARD_TITLE: &str = " Sifr ";

pub(crate) fn card_frame(color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(CARD_TITLE)
        .title_style(Style::default().fg(color))
}

impl<'a> CardView<'a> {
    pub(crate) fn title(&self) -> &'a str {
        self.displayed_track
            .map_or(NO_TRACK_TITLE, |track| track.title())
    }

    pub(crate) fn artist(&self) -> &'a str {
        self.displayed_track
            .and_then(|track| track.tags().artist.as_deref())
            .unwrap_or("")
    }

    pub(crate) fn progress_fraction(&self) -> f32 {
        let duration = self.duration();
        if duration.is_zero() {
            0.0
        } else {
            unit_fraction(self.position().div_duration_f64(duration))
        }
    }

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

    pub(crate) fn status(&self) -> CardStatus {
        CardStatus::new(*self.output_status, self.buffering_revision, self.player)
    }
}

impl<'a> CardWidget<'a> {
    #[must_use]
    pub(crate) fn new(view: CardView<'a>, active_theme: ActiveTheme<'a>) -> Self {
        Self {
            view,
            active_theme,
            cell_aspect: DEFAULT_CELL_ASPECT,
            cover_sizing: CoverSizing::default(),
            appearance_settings: AppearanceSettings::default(),
            card_cover: &CardCover::Missing,
            progress_bar_width: Cells(0),
            remaining_label: "",
        }
    }

    #[must_use]
    pub(crate) fn cell_aspect(mut self, cell_aspect: f32) -> Self {
        self.cell_aspect = cell_aspect;
        self
    }

    #[must_use]
    pub(crate) fn cover_sizing(mut self, cover_sizing: CoverSizing) -> Self {
        self.cover_sizing = cover_sizing;
        self
    }

    #[must_use]
    pub(crate) fn appearance_settings(
        mut self,
        appearance_settings: AppearanceSettings,
    ) -> Self {
        self.appearance_settings = appearance_settings;
        self
    }

    #[must_use]
    pub(crate) fn card_cover(mut self, card_cover: &'a CardCover) -> Self {
        self.card_cover = card_cover;
        self
    }

    #[must_use]
    pub(crate) fn progress_bar_width(mut self, progress_bar_width: Cells) -> Self {
        self.progress_bar_width = progress_bar_width;
        self
    }

    #[must_use]
    pub(crate) fn remaining_label(mut self, remaining_label: &'a str) -> Self {
        self.remaining_label = remaining_label;
        self
    }
}

impl CardWidget<'_> {
    pub(crate) fn paint(&self, metrics: &CardMetrics, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        let colors = self.active_theme.colors();
        let frame_color: Color = colors.muted_foreground;

        card_frame(frame_color)
            .padding(Inset::card().padding())
            .render(area, buffer);

        if !metrics.cover_square.is_empty() {
            self.paint_cover(buffer, metrics.cover_square);
        }
        headings::paint(buffer, self, metrics);
        meters::paint(buffer, self, metrics);
        if let Some(color) = self.bracket_color() {
            (&CornerBracketsWidget::new(color)).render(
                corner_brackets::expand(content_rect(metrics), BRACKET_MARGIN),
                buffer,
            );
        }
    }

    fn bracket_color(&self) -> Option<Color> {
        matches!(
            self.appearance_settings.cover_brackets,
            CoverBrackets::Shown
        )
        .then(|| self.active_theme.colors().accent)
    }

    fn paint_cover(&self, buffer: &mut Buffer, area: Rect) {
        match self.card_cover {
            CardCover::Missing => Paragraph::new(NO_COVER_TEXT)
                .style(Style::default().fg(self.active_theme.colors().muted_foreground))
                .alignment(Alignment::Center)
                .render(area, buffer),
            CardCover::Image => {}
            CardCover::Text(lines) => {
                let rows = small_count_u16(lines.len());
                let block = area.centered_vertically(Constraint::Length(rows));
                for (line, row) in lines.iter().zip(block.rows()) {
                    line.render(row, buffer);
                }
            }
        }
        if let Some(color) = self.bracket_color() {
            (&CornerBracketsWidget::new(color))
                .render(corner_brackets::expand(area, BRACKET_MARGIN), buffer);
        }
    }
}

impl Widget for &CardWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let metrics = CardMetrics::new(area, self.cell_aspect, self.cover_sizing);
        let remaining_label = remaining_label(self.view.remaining());
        let progress_bar_width = metrics.progress_bar_width(
            self.appearance_settings.progress_time,
            &remaining_label,
        );
        (*self)
            .progress_bar_width(progress_bar_width)
            .remaining_label(&remaining_label)
            .paint(&metrics, Canvas { area, buffer });
    }
}

#[must_use]
pub fn clock_frame_due(
    player: &Player,
    clock: Presence,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { playhead, .. } = player else {
        return None;
    };
    (clock == Presence::Shown).then(|| next_clock_second(*playhead, now))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        appearance::{AppearanceSettings, ProgressTime},
        bounded::Bounded,
        percent::Percent,
        player::{PausedBy, Player},
        playhead::Playhead,
        playlist::PlayOrder,
        revision::Revision,
        speed::Speed,
        time::Moment,
        track::Track,
        transport::{OutputError, OutputStatus},
    };
    use rstest::rstest;

    use crate::{
        card::{CardView, CardWidget, clock_frame_due, metrics::card_height},
        primitive::time_text::duration_text,
        repaint::Presence,
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
    };

    struct Fixture {
        player: Player,
        spectrum: Spectrum,
        output_status: OutputStatus,
        buffering_revision: Option<Revision>,
        play_order: PlayOrder,
        track: Option<Arc<Track>>,
    }

    impl Fixture {
        fn playing(track: Arc<Track>) -> Self {
            Self {
                player: Player::Playing {
                    track: Arc::clone(&track),
                    playhead: Playhead::anchored(
                        Duration::from_secs(30),
                        Moment::default(),
                        Speed::default(),
                    ),
                    preloaded: None,
                },
                spectrum: [0.5; SPECTRUM_BANDS],
                output_status: OutputStatus::Ready,
                buffering_revision: None,
                play_order: PlayOrder::default(),
                track: Some(track),
            }
        }

        fn output_lost(track: Arc<Track>) -> Self {
            Self {
                player: Player::Stopped,
                spectrum: [0.2; SPECTRUM_BANDS],
                output_status: OutputStatus::Lost(OutputError::DeviceGone),
                buffering_revision: None,
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
                repeat_mode: Default::default(),
                play_order: &self.play_order,
                displayed_track: self.track.as_ref(),
                output_status: &self.output_status,
                buffering_revision: self.buffering_revision,
                now: Moment::default(),
            }
        }
    }

    fn card<'a>(
        view: CardView<'a>,
        theme: &'a Theme,
        appearance_settings: AppearanceSettings,
    ) -> CardWidget<'a> {
        CardWidget::new(view, ActiveTheme::new(theme, ColorDepth::TrueColor))
            .appearance_settings(appearance_settings)
    }

    #[test]
    fn the_now_playing_card_shows_title_status_and_meters() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
        insta::assert_snapshot!(
            rendered(60, card_height().0, |frame| frame
                .render_widget(&widget, frame.area()))
            .to_string()
        );
    }

    #[test]
    fn a_stalled_download_shows_buffering_in_the_status_column() {
        let theme = noir();
        let fixture = Fixture {
            buffering_revision: Some(Revision::default()),
            ..Fixture::playing(track("Moon River"))
        };
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
        insta::assert_snapshot!(
            rendered(60, card_height().0, |frame| frame
                .render_widget(&widget, frame.area()))
            .to_string()
        );
    }

    #[test]
    fn output_lost_reads_no_output_in_the_status_column() {
        let theme = noir();
        let fixture = Fixture::output_lost(track("Moon River"));
        let widget = card(fixture.view(), &theme, AppearanceSettings::default());
        let text = rendered(60, card_height().0, |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("No output"), "got {text:?}");
    }

    #[test]
    fn progress_time_remaining_shows_a_countdown_chip() {
        let theme = noir();
        let fixture = Fixture::playing(track("Moon River"));
        let appearance_settings = AppearanceSettings {
            progress_time: ProgressTime::Remaining,
            ..AppearanceSettings::default()
        };
        let widget = card(fixture.view(), &theme, appearance_settings);
        let text = rendered(60, card_height().0, |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        let remaining = format!("-{}", duration_text(Duration::from_secs(245 - 30)));
        assert!(text.contains(&remaining), "got {text:?}");
    }

    fn clock_player() -> Player {
        Player::Playing {
            track: track("Moon River"),
            playhead: Playhead::anchored(
                Duration::from_secs(10),
                Moment::default(),
                Speed::clamped(1.0),
            ),
            preloaded: None,
        }
    }

    fn paused_clock_player() -> Player {
        Player::Paused {
            track: track("Moon River"),
            position: Duration::from_secs(10),
            by: PausedBy::Listener,
        }
    }

    #[rstest]
    #[case::a_playing_clock_wants_the_next_second(
        clock_player(),
        Presence::Shown,
        Some(Duration::from_millis(1_001))
    )]
    #[case::a_hidden_clock_wants_no_frame(clock_player(), Presence::Hidden, None)]
    #[case::a_paused_clock_wants_no_frame(paused_clock_player(), Presence::Shown, None)]
    fn clock_frame_due_wants_the_next_second_only_while_a_shown_clock_plays(
        #[case] player: Player,
        #[case] clock: Presence,
        #[case] due_after: Option<Duration>,
    ) {
        assert_eq!(
            clock_frame_due(&player, clock, Moment::default()),
            due_after.map(Moment::new)
        );
    }
}
