use kernel::{Moment, Player};
use runtime::{FRAME_INTERVAL, FrameDue};
use terminal::CoverMotion;
use widgets::{
    AnimationStage,
    Presence,
    ProgressScale,
    SpectrumMotion,
    next_clock_second,
    next_progress_step,
    next_sleep_minute,
};

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

pub(crate) fn animation_frame_due(effect: FrameEffect, last_paint: Moment) -> FrameDue {
    match effect {
        FrameEffect::Live => {
            FrameDue::At(Moment::new(last_paint.since_epoch() + FRAME_INTERVAL))
        }
        FrameEffect::Settled => FrameDue::Settled,
    }
}

pub(crate) fn progress_frame_due(
    player: &Player,
    bar: Option<u16>,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { head, track, .. } = player else {
        return None;
    };
    let scale = ProgressScale::text_bar(bar?, track.duration()?)?;
    next_progress_step(scale, *head, now)
}

pub(crate) fn clock_frame_due(
    player: &Player,
    clock: Presence,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { head, .. } = player else {
        return None;
    };
    (clock == Presence::Shown).then(|| next_clock_second(*head, now))
}

pub(crate) fn sleep_frame_due(
    deadline: Option<Moment>,
    label: Presence,
    now: Moment,
) -> Option<Moment> {
    if label != Presence::Shown {
        return None;
    }
    next_sleep_minute(deadline?, now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClockState {
    Playing,
    Halted,
}

impl ClockState {
    pub(crate) fn of(player: &Player) -> Self {
        match player {
            Player::Playing { .. } => Self::Playing,
            Player::Stopped | Player::Loading { .. } | Player::Paused { .. } => {
                Self::Halted
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SpectrumSources {
    pub(crate) player: ClockState,
    pub(crate) shown: Presence,
    pub(crate) motion: SpectrumMotion,
    pub(crate) last_paint: Moment,
}

pub(crate) fn spectrum_frame_due(sources: SpectrumSources, now: Moment) -> FrameDue {
    let _ = now;
    let wants_frame = matches!(
        (sources.shown, sources.player, sources.motion),
        (Presence::Shown, ClockState::Playing, _)
            | (Presence::Shown, ClockState::Halted, SpectrumMotion::Moving)
    );
    if wants_frame {
        FrameDue::At(Moment::new(
            sources.last_paint.since_epoch() + FRAME_INTERVAL,
        ))
    } else {
        FrameDue::Settled
    }
}

pub(crate) fn earliest(first: FrameDue, second: FrameDue) -> FrameDue {
    match (first, second) {
        (FrameDue::At(first), FrameDue::At(second)) => FrameDue::At(first.min(second)),
        (FrameDue::At(at), FrameDue::Settled)
        | (FrameDue::Settled, FrameDue::At(at)) => FrameDue::At(at),
        (FrameDue::Settled, FrameDue::Settled) => FrameDue::Settled,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::{
        AudioFormat,
        Bounded,
        Moment,
        Pause,
        Player,
        Playhead,
        Preload,
        Speed,
        Tags,
        Track,
    };
    use rstest::rstest;
    use runtime::FrameDue;
    use terminal::CoverMotion;
    use widgets::{
        AnimationStage,
        Presence,
        ProgressScale,
        SPECTRUM_BANDS,
        SpectrumMotion,
        SpectrumSmoothing,
        next_progress_step,
    };

    use crate::shell::frame_clock::{
        ClockState,
        FrameEffect,
        SpectrumSources,
        animation_frame_due,
        clock_frame_due,
        earliest,
        frame_effect,
        progress_frame_due,
        sleep_frame_due,
        spectrum_frame_due,
    };

    fn track(duration: Duration) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path("/music/song.mp3")
                .duration(duration)
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    fn playing(offset: Duration, since: Moment, duration: Duration) -> Player {
        Player::Playing {
            track: track(duration),
            head: Playhead::anchored(offset, since, Speed::clamped(1.0)),
            preload: Preload::None,
        }
    }

    fn paused(at: Duration, duration: Duration) -> Player {
        Player::Paused {
            track: track(duration),
            at,
            pause: Pause::ByListener,
        }
    }

    #[rstest]
    #[case::a_stopped_player_has_no_progress_frame(Player::Stopped, Some(50), None)]
    #[case::a_paused_player_has_no_progress_frame(
        paused(Duration::from_secs(10), Duration::from_secs(100)),
        Some(50),
        None
    )]
    #[case::a_playing_track_wants_the_next_progress_step(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            Duration::from_secs(100)
        ),
        Some(50),
        Some(Moment::new(Duration::from_millis(100_801)))
    )]
    #[case::no_bar_has_no_progress_frame(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            Duration::from_secs(100)
        ),
        None,
        None
    )]
    #[case::a_sped_up_track_still_wants_a_progress_step(
        Player::Playing {
            track: track(Duration::from_secs(100)),
            head: Playhead::anchored(
                Duration::from_millis(10_200),
                Moment::new(Duration::from_secs(100)),
                Speed::clamped(1.5)
            ),
            preload: Preload::None,
        },
        Some(50),
        next_progress_step(
            ProgressScale::text_bar(50, Duration::from_secs(100)).unwrap(),
            Playhead::anchored(
                Duration::from_millis(10_200),
                Moment::new(Duration::from_secs(100)),
                Speed::clamped(1.5)
            ),
            Moment::new(Duration::from_secs(100))
        )
    )]
    fn a_progress_frame_is_due_only_while_the_bar_can_move(
        #[case] player: Player,
        #[case] bar: Option<u16>,
        #[case] expected: Option<Moment>,
    ) {
        let now = Moment::new(Duration::from_secs(100));

        assert_eq!(progress_frame_due(&player, bar, now), expected);
    }

    #[test]
    fn a_playing_clock_wants_the_next_second() {
        let now = Moment::new(Duration::from_secs(100));
        let player = playing(Duration::from_secs(10), now, Duration::from_secs(100));

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
        let player = playing(Duration::from_secs(10), now, Duration::from_secs(100));

        assert_eq!(clock_frame_due(&player, Presence::Hidden, now), None);
    }

    #[test]
    fn a_paused_clock_wants_no_frame() {
        let now = Moment::new(Duration::from_secs(100));
        let player = paused(Duration::from_secs(10), Duration::from_secs(100));

        assert_eq!(clock_frame_due(&player, Presence::Shown, now), None);
    }

    #[test]
    fn a_paused_player_with_a_sleep_timer_wakes_once_a_minute() {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline =
            Moment::new(now.since_epoch() + Duration::from_secs(14 * 60 + 59));
        let player = paused(Duration::from_secs(10), Duration::from_secs(100));

        assert_eq!(clock_frame_due(&player, Presence::Shown, now), None);
        assert_eq!(
            sleep_frame_due(Some(deadline), Presence::Shown, now),
            Some(Moment::new(now.since_epoch() + Duration::from_secs(59)))
        );
    }

    #[test]
    fn a_hidden_sleep_label_wants_no_frame() {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline = Moment::new(now.since_epoch() + Duration::from_secs(60));

        assert_eq!(sleep_frame_due(Some(deadline), Presence::Hidden, now), None);
    }

    #[test]
    fn no_deadline_wants_no_sleep_frame() {
        let now = Moment::new(Duration::from_secs(1_000));

        assert_eq!(sleep_frame_due(None, Presence::Shown, now), None);
    }

    #[rstest]
    #[case::progress_wins(
        [
            Some(Duration::from_millis(400)),
            Some(Duration::from_millis(800)),
            Some(Duration::from_secs(40)),
        ],
        Some(Duration::from_millis(400))
    )]
    #[case::nothing_moves([None, None, None], None)]
    fn the_earliest_source_wins(
        #[case] sources: [Option<Duration>; 3],
        #[case] expected: Option<Duration>,
    ) {
        let now = Moment::new(Duration::from_secs(100));
        let due = |offset: Option<Duration>| {
            offset.map_or(FrameDue::Settled, |delta| {
                FrameDue::At(Moment::new(now.since_epoch() + delta))
            })
        };

        let combined = sources
            .into_iter()
            .fold(FrameDue::Settled, |acc, source| earliest(acc, due(source)));

        assert_eq!(combined, due(expected));
    }

    #[test]
    fn a_settled_deadline_yields_to_a_real_one() {
        let reference = Moment::new(Duration::from_secs(1));
        let due = FrameDue::At(reference);

        assert_eq!(earliest(due, FrameDue::Settled), due);
        assert_eq!(earliest(FrameDue::Settled, due), due);
    }

    #[test]
    fn two_settled_deadlines_stay_settled() {
        assert_eq!(
            earliest(FrameDue::Settled, FrameDue::Settled),
            FrameDue::Settled
        );
    }

    #[test]
    fn a_live_effect_wants_a_frame_at_the_interval_after_the_last_paint() {
        let last_paint = Moment::new(Duration::from_secs(1));

        assert_eq!(
            animation_frame_due(FrameEffect::Live, last_paint),
            FrameDue::At(Moment::new(
                last_paint.since_epoch() + runtime::FRAME_INTERVAL
            ))
        );
    }

    #[test]
    fn the_deadline_does_not_slide_across_repeated_calls() {
        let last_paint = Moment::new(Duration::from_secs(1));

        let first = animation_frame_due(FrameEffect::Live, last_paint);
        let second = animation_frame_due(FrameEffect::Live, last_paint);

        assert_eq!(first, second);
    }

    #[test]
    fn no_live_effect_wants_no_frame() {
        let last_paint = Moment::new(Duration::from_secs(1));

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

    fn spectrum_sources(
        player: ClockState,
        shown: Presence,
        motion: SpectrumMotion,
    ) -> SpectrumSources {
        SpectrumSources {
            player,
            shown,
            motion,
            last_paint: Moment::new(Duration::from_secs(10)),
        }
    }

    #[rstest]
    #[case::playing_shown_settled(
        spectrum_sources(
            ClockState::Playing,
            Presence::Shown,
            SpectrumMotion::Settled
        ),
        true
    )]
    #[case::playing_hidden_moving(
        spectrum_sources(
            ClockState::Playing,
            Presence::Hidden,
            SpectrumMotion::Moving
        ),
        false
    )]
    #[case::halted_shown_moving(
        spectrum_sources(ClockState::Halted, Presence::Shown, SpectrumMotion::Moving),
        true
    )]
    #[case::halted_shown_settled(
        spectrum_sources(ClockState::Halted, Presence::Shown, SpectrumMotion::Settled),
        false
    )]
    #[case::halted_hidden_moving(
        spectrum_sources(ClockState::Halted, Presence::Hidden, SpectrumMotion::Moving),
        false
    )]
    fn a_spectrum_frame_is_due_only_while_bands_can_move(
        #[case] sources: SpectrumSources,
        #[case] wants_frame: bool,
    ) {
        let now = Moment::new(Duration::from_secs(10));
        let expected = if wants_frame {
            FrameDue::At(Moment::new(
                sources.last_paint.since_epoch() + runtime::FRAME_INTERVAL,
            ))
        } else {
            FrameDue::Settled
        };

        assert_eq!(spectrum_frame_due(sources, now), expected);
    }

    #[test]
    fn a_paused_spectrum_decays_to_settled() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::from_secs(10));
        let mut frames = 0;
        while smoothing.motion() == SpectrumMotion::Moving && frames < 300 {
            let _ = smoothing.fade(Duration::from_millis(33));
            frames += 1;
        }

        assert!(frames < 300);
        let sources = SpectrumSources {
            player: ClockState::Halted,
            shown: Presence::Shown,
            motion: smoothing.motion(),
            last_paint: Moment::new(Duration::from_secs(100)),
        };

        assert_eq!(
            spectrum_frame_due(sources, Moment::new(Duration::from_secs(100))),
            FrameDue::Settled
        );
    }
}
