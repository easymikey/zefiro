use std::time::{Duration, Instant};

use runtime::FrameDue;
use terminal::CoverMotion;
use widgets::AnimationStage;

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameEffect {
    Live,
    Settled,
}

pub(crate) fn frame_effect(
    stage: &AnimationStage,
    cover_motion: CoverMotion,
) -> FrameEffect {
    if stage.wants_frame() || cover_motion == CoverMotion::Crossfading {
        FrameEffect::Live
    } else {
        FrameEffect::Settled
    }
}

pub(crate) fn animation_frame_due(
    effect: FrameEffect,
    last_paint: Instant,
) -> FrameDue {
    match effect {
        FrameEffect::Live => FrameDue::At(last_paint + FRAME_INTERVAL),
        FrameEffect::Settled => FrameDue::Settled,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use runtime::FrameDue;
    use terminal::CoverMotion;
    use widgets::AnimationStage;

    use crate::shell::frame_clock::{
        FRAME_INTERVAL,
        FrameEffect,
        animation_frame_due,
        frame_effect,
    };

    #[test]
    fn a_live_effect_wants_a_frame_at_the_interval_after_the_last_paint() {
        let last_paint = Instant::now();

        assert_eq!(
            animation_frame_due(FrameEffect::Live, last_paint),
            FrameDue::At(last_paint + FRAME_INTERVAL)
        );
    }

    #[test]
    fn the_deadline_does_not_slide_across_repeated_calls() {
        let last_paint = Instant::now();

        let first = animation_frame_due(FrameEffect::Live, last_paint);
        let second = animation_frame_due(FrameEffect::Live, last_paint);

        assert_eq!(first, second);
    }

    #[test]
    fn no_live_effect_wants_no_frame() {
        let last_paint = Instant::now();

        assert_eq!(
            animation_frame_due(FrameEffect::Settled, last_paint),
            FrameDue::Settled
        );
    }

    #[test]
    fn a_running_crossfade_wants_a_frame_at_the_interval() {
        let stage = AnimationStage::default();

        assert_eq!(
            frame_effect(&stage, CoverMotion::Crossfading),
            FrameEffect::Live
        );
    }

    #[test]
    fn a_settled_crossfade_wants_no_frame() {
        let stage = AnimationStage::default();

        assert_eq!(
            frame_effect(&stage, CoverMotion::Still),
            FrameEffect::Settled
        );
    }
}
