use std::time::Duration;

pub const FRAME_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repaint {
    Settled,
    Now,
    NextFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepaintCause {
    Input,
    Event,
}

pub(crate) fn repaint_after(current: Repaint, source: RepaintCause) -> Repaint {
    match (current, source) {
        (_, RepaintCause::Input) | (Repaint::Now, RepaintCause::Event) => Repaint::Now,
        (Repaint::Settled | Repaint::NextFrame, RepaintCause::Event) => {
            Repaint::NextFrame
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::repaint::{Repaint, RepaintCause, repaint_after};

    #[rstest]
    #[case::settled_input_is_now(Repaint::Settled, RepaintCause::Input, Repaint::Now)]
    #[case::settled_fact_waits(
        Repaint::Settled,
        RepaintCause::Event,
        Repaint::NextFrame
    )]
    #[case::frame_input_is_now(Repaint::NextFrame, RepaintCause::Input, Repaint::Now)]
    #[case::now_fact_stays_now(Repaint::Now, RepaintCause::Event, Repaint::Now)]
    #[case::frame_fact_stays_frame(
        Repaint::NextFrame,
        RepaintCause::Event,
        Repaint::NextFrame
    )]
    fn repaint_after_escalates_by_source_and_current(
        #[case] current: Repaint,
        #[case] source: RepaintCause,
        #[case] expected: Repaint,
    ) {
        assert_eq!(repaint_after(current, source), expected);
    }
}
