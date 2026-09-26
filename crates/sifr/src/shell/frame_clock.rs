use std::time::{Duration, Instant};

use kernel::{Moment, Player};
use runtime::FrameDue;
use terminal::CoverMotion;
use widgets::{AnimationStage, next_clock_second};

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

pub(crate) fn playhead_frame_due(
    player: &Player,
    now: Moment,
    reference: Instant,
) -> FrameDue {
    let Player::Playing { head, .. } = player else {
        return FrameDue::Settled;
    };
    let due = next_clock_second(*head, now);
    FrameDue::At(reference + due.elapsed_since(now))
}

pub(crate) fn earliest_frame_due(first: FrameDue, second: FrameDue) -> FrameDue {
    match (first, second) {
        (FrameDue::At(first), FrameDue::At(second)) => FrameDue::At(first.min(second)),
        (FrameDue::At(at), FrameDue::Settled)
        | (FrameDue::Settled, FrameDue::At(at)) => FrameDue::At(at),
        (FrameDue::Settled, FrameDue::Settled) => FrameDue::Settled,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use kernel::{AudioFormat, Moment, Player, Playhead, Preload, Speed, Tags, Track};
    use runtime::FrameDue;
    use terminal::CoverMotion;
    use widgets::AnimationStage;

    use crate::shell::frame_clock::{
        FRAME_INTERVAL,
        FrameEffect,
        animation_frame_due,
        earliest_frame_due,
        frame_effect,
        playhead_frame_due,
    };

    fn playing(offset: Duration, since: Moment) -> Player {
        let track = Arc::new(
            Track::builder()
                .path("/music/song.mp3")
                .duration(Duration::from_secs(245))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        );
        Player::Playing {
            track,
            head: Playhead::anchored(offset, since, Speed::default()),
            preload: Preload::None,
        }
    }

    #[test]
    fn a_stopped_player_wants_no_motion_frame() {
        let reference = Instant::now();
        let now = Moment::new(Duration::from_secs(10));

        assert_eq!(
            playhead_frame_due(&Player::Stopped, now, reference),
            FrameDue::Settled
        );
    }

    #[test]
    fn a_playing_player_wants_a_frame_at_the_next_whole_second() {
        let reference = Instant::now();
        let now = Moment::new(Duration::from_secs(10));
        let player = playing(Duration::from_secs(10), now);

        assert_eq!(
            playhead_frame_due(&player, now, reference),
            FrameDue::At(reference + Duration::from_millis(1_001))
        );
    }

    #[test]
    fn the_earlier_of_two_deadlines_wins() {
        let reference = Instant::now();
        let sooner = FrameDue::At(reference);
        let later = FrameDue::At(reference + FRAME_INTERVAL);

        assert_eq!(earliest_frame_due(sooner, later), sooner);
        assert_eq!(earliest_frame_due(later, sooner), sooner);
    }

    #[test]
    fn a_settled_deadline_yields_to_a_real_one() {
        let reference = Instant::now();
        let due = FrameDue::At(reference);

        assert_eq!(earliest_frame_due(due, FrameDue::Settled), due);
        assert_eq!(earliest_frame_due(FrameDue::Settled, due), due);
    }

    #[test]
    fn two_settled_deadlines_stay_settled() {
        assert_eq!(
            earliest_frame_due(FrameDue::Settled, FrameDue::Settled),
            FrameDue::Settled
        );
    }

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
