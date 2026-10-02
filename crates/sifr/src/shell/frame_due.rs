use kernel::{Moment, Player};
use runtime::FRAME_INTERVAL;
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

use crate::shell::motion::{Motion, SpectrumFeed};

pub(crate) fn animation_frame_due(
    stage: &AnimationStage,
    cover_motion: CoverMotion,
    last_paint: Moment,
) -> Option<Moment> {
    (stage.is_animating() || cover_motion == CoverMotion::Animating)
        .then(|| next_frame(last_paint))
}

fn next_frame(last_paint: Moment) -> Moment {
    Moment::new(last_paint.since_epoch() + FRAME_INTERVAL)
}

pub(crate) fn progress_frame_due(
    player: &Player,
    bar_width: Option<u16>,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { head, track, .. } = player else {
        return None;
    };
    let scale = ProgressScale::text_bar(bar_width?, track.duration()?)?;
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

pub(crate) fn spectrum_frame_due(
    motion: &Motion,
    feed: SpectrumFeed,
) -> Option<Moment> {
    let wants_frame = matches!(
        (motion.on_screen.spectrum, feed, motion.spectrum_motion),
        (Presence::Shown, SpectrumFeed::Live, _)
            | (
                Presence::Shown,
                SpectrumFeed::Silent,
                SpectrumMotion::Moving
            )
    );
    wants_frame.then(|| next_frame(motion.last_paint))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::{
        AudioFormat,
        Bounded,
        Moment,
        PausedBy,
        Player,
        Playhead,
        Preload,
        Speed,
        Tags,
        Track,
    };
    use rstest::rstest;
    use terminal::CoverMotion;
    use widgets::{
        AnimationStage,
        OnScreen,
        Presence,
        ProgressScale,
        SPECTRUM_BANDS,
        SpectrumMotion,
        SpectrumSmoothing,
        next_progress_step,
    };

    use crate::shell::{
        frame_due::{
            animation_frame_due,
            clock_frame_due,
            progress_frame_due,
            sleep_frame_due,
            spectrum_frame_due,
        },
        motion::{Motion, SpectrumFeed},
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
            by: PausedBy::Listener,
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
        #[case] bar_width: Option<u16>,
        #[case] expected: Option<Moment>,
    ) {
        let now = Moment::new(Duration::from_secs(100));

        assert_eq!(progress_frame_due(&player, bar_width, now), expected);
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

    #[test]
    fn a_running_crossfade_wants_a_frame_at_the_interval_after_the_last_paint() {
        let stage = AnimationStage::default();
        let last_paint = Moment::new(Duration::from_secs(1));

        assert_eq!(
            animation_frame_due(&stage, CoverMotion::Animating, last_paint),
            Some(Moment::new(
                last_paint.since_epoch() + runtime::FRAME_INTERVAL
            ))
        );
    }

    #[test]
    fn a_settled_crossfade_wants_no_frame() {
        let stage = AnimationStage::default();
        let last_paint = Moment::new(Duration::from_secs(1));

        assert_eq!(
            animation_frame_due(&stage, CoverMotion::Still, last_paint),
            None
        );
    }

    fn motion_with_spectrum(
        spectrum: Presence,
        spectrum_motion: SpectrumMotion,
    ) -> Motion {
        Motion {
            on_screen: OnScreen {
                progress_bar: None,
                clock: Presence::Hidden,
                sleep_label: Presence::Hidden,
                spectrum,
            },
            spectrum_motion,
            last_paint: Moment::new(Duration::from_secs(10)),
            ..Motion::default()
        }
    }

    #[rstest]
    #[case::playing_shown_settled(
        SpectrumFeed::Live,
        motion_with_spectrum(Presence::Shown, SpectrumMotion::Settled),
        true
    )]
    #[case::playing_hidden_moving(
        SpectrumFeed::Live,
        motion_with_spectrum(Presence::Hidden, SpectrumMotion::Moving),
        false
    )]
    #[case::halted_shown_moving(
        SpectrumFeed::Silent,
        motion_with_spectrum(Presence::Shown, SpectrumMotion::Moving),
        true
    )]
    #[case::halted_shown_settled(
        SpectrumFeed::Silent,
        motion_with_spectrum(Presence::Shown, SpectrumMotion::Settled),
        false
    )]
    #[case::halted_hidden_moving(
        SpectrumFeed::Silent,
        motion_with_spectrum(Presence::Hidden, SpectrumMotion::Moving),
        false
    )]
    fn a_spectrum_frame_is_due_only_while_bands_can_move(
        #[case] feed: SpectrumFeed,
        #[case] motion: Motion,
        #[case] wants_frame: bool,
    ) {
        let expected = wants_frame.then(|| {
            Moment::new(motion.last_paint.since_epoch() + runtime::FRAME_INTERVAL)
        });

        assert_eq!(spectrum_frame_due(&motion, feed), expected);
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
        let motion = motion_with_spectrum(Presence::Shown, smoothing.motion());

        assert_eq!(spectrum_frame_due(&motion, SpectrumFeed::Silent), None);
    }
}
