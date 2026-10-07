use kernel::domain::{appearance::SpeedChip, geometry::Cells, playlist::RepeatMode};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    card::CardView,
    primitive::{
        bar::BarFill,
        chip::{speed_chip_spans, speed_chip_width},
        span::{line, text},
        time_text::{elapsed_text, elapsed_width},
        truncate::truncate_owned,
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MinimalScreenWidget<'a> {
    view: CardView<'a>,
    theme: ActiveTheme<'a>,
    speed_chip: SpeedChip,
    progress_bar_width: Cells,
}

impl<'a> MinimalScreenWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        view: CardView<'a>,
        active_theme: ActiveTheme<'a>,
        progress_bar_width: Cells,
    ) -> Self {
        Self {
            view,
            theme: active_theme,
            speed_chip: SpeedChip::default(),
            progress_bar_width,
        }
    }

    #[must_use]
    pub(crate) fn speed_chip(mut self, speed_chip: SpeedChip) -> Self {
        self.speed_chip = speed_chip;
        self
    }
}

impl Widget for &MinimalScreenWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let rows = [
            self.title_line(Cells(area.width)),
            self.progress_line(Cells(area.width)),
            self.status_line(Cells(area.width)),
        ];
        for (offset, line) in (0..area.height).zip(rows) {
            Paragraph::new(line).render(
                Rect {
                    y: area.y + offset,
                    height: 1,
                    ..area
                },
                buffer,
            );
        }
    }
}

fn time_width(view: CardView<'_>, width: Cells) -> u16 {
    elapsed_width(view.position(), view.duration()).min(width.0)
}

#[must_use]
pub(crate) fn progress_bar_width(
    view: CardView<'_>,
    speed_chip: SpeedChip,
    width: Cells,
) -> Cells {
    let time_width = time_width(view, width);
    let gap = u16::from(width.0 > time_width);
    let chip_width = speed_chip_width(view.speed, speed_chip);
    Cells(width.0.saturating_sub(time_width + gap + chip_width.0))
}

impl MinimalScreenWidget<'_> {
    fn title_line(&self, width: Cells) -> Line<'static> {
        let status = self.view.status();
        let color = status.color(&self.theme);
        let title = self.view.title();
        let label = format!("{} {title}", status.label().glyph);
        line([text(truncate_owned(label, width.count())).fg(color)])
    }

    fn progress_line(&self, width: Cells) -> Line<'static> {
        let colors = self.theme.colors();
        let bar_width = self.progress_bar_width;
        let mut spans = BarFill::progress(self.view.progress_fraction(), bar_width)
            .line(self.theme.progress_fill(), self.theme.progress_groove())
            .spans;
        if bar_width > Cells(0) {
            spans.push(Span::raw(" "));
        }
        let elapsed = elapsed_text(self.view.position(), self.view.duration());
        let elapsed =
            truncate_owned(elapsed, usize::from(time_width(self.view, width)));
        spans.push(text(elapsed).fg(colors.foreground).into());
        spans.extend(speed_chip_spans(self.view.speed, self.speed_chip, &colors));
        Line::from(spans)
    }

    fn status_line(&self, width: Cells) -> Line<'static> {
        let repeat = match self.view.repeat_mode {
            RepeatMode::Off => "Off",
            RepeatMode::All => "All",
            RepeatMode::One => "One",
        };
        let shuffle = if self.view.play_order.is_shuffle() {
            "On"
        } else {
            "Off"
        };
        let status = format!(
            "Vol {}%  Shuf {shuffle}  Rep {repeat}",
            self.view.volume.get()
        );
        Line::from(Span::styled(
            truncate_owned(status, width.count()),
            Style::default().fg(self.theme.colors().muted_foreground),
        ))
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::SpeedChip,
        bounded::Bounded,
        geometry::Cells,
        percent::Percent,
        player::Player,
        playlist::{PlayOrder, RepeatMode},
        speed::Speed,
        time::Moment,
        transport::OutputStatus,
    };
    use rstest::rstest;

    use crate::{
        card::CardView,
        primitive::{chip::speed_chip_width, time_text::elapsed_text},
        screen::minimal::{MinimalScreenWidget, progress_bar_width},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn painted(repeat_mode: RepeatMode) -> String {
        let theme = noir();
        let player = Player::Stopped;
        let spectrum: Spectrum = [0.0; SPECTRUM_BANDS];
        let output_status = OutputStatus::Ready;
        let play_order = PlayOrder::default();
        let view = CardView {
            player: &player,
            speed: Speed::default(),
            volume: Percent::clamped(50),
            spectrum: &spectrum,
            repeat_mode,
            play_order: &play_order,
            displayed_track: None,
            output_status: &output_status,
            now: Moment::default(),
        };
        let widget = MinimalScreenWidget::new(
            view,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
            progress_bar_width(view, SpeedChip::Always, Cells(40)),
        )
        .speed_chip(SpeedChip::Always);
        rendered(40, 3, |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string()
    }

    #[test]
    fn the_bar_leaves_room_for_the_elapsed_text_a_gap_and_the_speed_chip() {
        let player = Player::Stopped;
        let spectrum: Spectrum = [0.0; SPECTRUM_BANDS];
        let output_status = OutputStatus::Ready;
        let play_order = PlayOrder::default();
        let view = CardView {
            player: &player,
            speed: Speed::default(),
            volume: Percent::clamped(50),
            spectrum: &spectrum,
            repeat_mode: RepeatMode::Off,
            play_order: &play_order,
            displayed_track: None,
            output_status: &output_status,
            now: Moment::default(),
        };
        let elapsed = elapsed_text(view.position(), view.duration());
        let chip = speed_chip_width(view.speed, SpeedChip::Always);
        let bar = progress_bar_width(view, SpeedChip::Always, Cells(40));
        assert_eq!(
            usize::from(bar.0 + chip.0) + 1 + elapsed.chars().count(),
            40,
            "{elapsed:?}"
        );
    }

    #[rstest]
    #[case::off(RepeatMode::Off, "Shuf Off  Rep Off")]
    #[case::all(RepeatMode::All, "Rep All")]
    #[case::one(RepeatMode::One, "Rep One")]
    fn minimal_repeat_mode_label_is_capitalised(
        #[case] repeat_mode: RepeatMode,
        #[case] expected: &str,
    ) {
        let text = painted(repeat_mode);
        assert!(text.contains(expected), "got {text:?}");
    }
}
