use std::time::Duration;

pub const FRAME: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repaint {
    Settled,
    Now,
    Frame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Input,
    Fact,
}

pub(crate) fn repaint_after(current: Repaint, source: Source) -> Repaint {
    match (current, source) {
        (_, Source::Input) | (Repaint::Now, Source::Fact) => Repaint::Now,
        (Repaint::Settled | Repaint::Frame, Source::Fact) => Repaint::Frame,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::repaint::{Repaint, Source, repaint_after};

    #[rstest]
    #[case::settled_input_is_now(Repaint::Settled, Source::Input, Repaint::Now)]
    #[case::settled_fact_waits(Repaint::Settled, Source::Fact, Repaint::Frame)]
    #[case::frame_input_is_now(Repaint::Frame, Source::Input, Repaint::Now)]
    #[case::now_fact_stays_now(Repaint::Now, Source::Fact, Repaint::Now)]
    #[case::frame_fact_stays_frame(Repaint::Frame, Source::Fact, Repaint::Frame)]
    fn repaint_after_escalates_by_source_and_current(
        #[case] current: Repaint,
        #[case] source: Source,
        #[case] expected: Repaint,
    ) {
        assert_eq!(repaint_after(current, source), expected);
    }
}
