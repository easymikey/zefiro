use std::time::Duration;

use kernel::domain::time::Moment;
use ratatui::style::Color;

use crate::{
    primitive::span::{StyledText, text},
    repaint::Presence,
    theme::colors::Colors,
};

const STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spinner {
    since_first_paint: Duration,
}

impl Spinner {
    #[must_use]
    pub fn new(since_first_paint: Duration) -> Self {
        Self { since_first_paint }
    }

    #[must_use]
    pub(crate) fn glyph(self) -> &'static str {
        let tenths = self.since_first_paint.as_secs() % 4 * 10
            + u64::from(self.since_first_paint.subsec_millis() / 100);
        match tenths % 8 {
            0 => "⣾",
            1 => "⣽",
            2 => "⣻",
            3 => "⢿",
            4 => "⡿",
            5 => "⣟",
            6 => "⣯",
            _ => "⣷",
        }
    }

    #[must_use]
    pub(crate) fn mark(self, colors: &Colors<Color>) -> [StyledText<'static>; 2] {
        [text(self.glyph()).fg(colors.accent), text(" ")]
    }

    #[must_use]
    pub fn frame_due(self, presence: Presence, now: Moment) -> Option<Moment> {
        let phase = u64::from(self.since_first_paint.subsec_millis() % 100);
        (presence == Presence::Shown).then(|| {
            Moment::new(now.since_epoch() + STEP - Duration::from_millis(phase))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::time::Moment;
    use rstest::rstest;

    use crate::{primitive::spinner::Spinner, repaint::Presence};

    #[rstest]
    #[case::the_first_paint(0, "⣾")]
    #[case::the_end_of_the_first_step(99, "⣾")]
    #[case::the_second_step(100, "⣽")]
    #[case::the_third_step(250, "⣻")]
    #[case::the_last_step(750, "⣷")]
    #[case::the_ninth_step_starts_over(800, "⣾")]
    #[case::four_seconds_on(4_250, "⣻")]
    fn the_spinner_steps_through_the_dot_frames_ten_times_a_second(
        #[case] millis: u64,
        #[case] expected: &str,
    ) {
        assert_eq!(
            Spinner::new(Duration::from_millis(millis)).glyph(),
            expected
        );
    }

    #[rstest]
    #[case::a_shown_wait_asks_the_next_step(Presence::Shown, Some(1_050))]
    #[case::no_wait_on_screen_asks_no_frame(Presence::Hidden, None)]
    fn the_spinner_asks_a_frame_at_its_next_step_only_while_shown(
        #[case] presence: Presence,
        #[case] expected: Option<u64>,
    ) {
        let spinner = Spinner::new(Duration::from_millis(1_150));
        assert_eq!(
            spinner.frame_due(presence, Moment::new(Duration::from_millis(1_000))),
            expected.map(|millis| Moment::new(Duration::from_millis(millis)))
        );
    }
}
