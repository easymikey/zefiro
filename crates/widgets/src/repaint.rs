use std::{num::NonZeroU32, time::Duration};

use kernel::domain::{
    geometry::Cells,
    player::Player,
    playhead::Playhead,
    time::{Moment, SECONDS_PER_MINUTE},
};

const STEP_CORRECTION: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Shown,
    Hidden,
}

impl From<bool> for Presence {
    fn from(visible: bool) -> Self {
        if visible { Self::Shown } else { Self::Hidden }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnScreen {
    pub progress_bar_width: Option<Cells>,
    pub clock: Presence,
    pub sleep_label: Presence,
    pub spectrum: Presence,
    pub spinner: Presence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProgressScale {
    steps: NonZeroU32,
    duration: Duration,
}

impl ProgressScale {
    fn text_bar(width: Cells, duration: Duration) -> Option<Self> {
        let steps = NonZeroU32::new(2 * u32::from(width.0))?;
        Some(Self { steps, duration })
    }
}

fn next_progress_step(
    scale: ProgressScale,
    playhead: Playhead,
    now: Moment,
) -> Option<Moment> {
    let step_length = scale.duration / scale.steps.get();
    if step_length.is_zero() {
        return None;
    }
    let position = playhead.position_at(now);
    let step_index = duration_steps(position, step_length);
    let next_position = step_length.saturating_mul(step_index.saturating_add(1));
    if next_position > scale.duration {
        return None;
    }
    Some(wall_moment(playhead, next_position))
}

#[must_use]
pub fn progress_frame_due(
    player: &Player,
    bar_width: Option<Cells>,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing {
        playhead, track, ..
    } = player
    else {
        return None;
    };
    let scale = ProgressScale::text_bar(bar_width?, track.duration()?)?;
    next_progress_step(scale, *playhead, now)
}

#[must_use]
pub fn next_clock_second(playhead: Playhead, now: Moment) -> Moment {
    let position = playhead.position_at(now);
    let next_position = Duration::from_secs(position.as_secs() + 1);
    wall_moment(playhead, next_position)
}

#[must_use]
pub(crate) fn next_sleep_minute(deadline_at: Moment, now: Moment) -> Option<Moment> {
    if now >= deadline_at {
        return None;
    }
    let left = deadline_at.elapsed_since(now);
    let minutes = ceil_minutes(left);
    let next_position = Duration::from_secs((minutes - 1) * SECONDS_PER_MINUTE);
    Some(Moment::new(
        deadline_at.since_epoch().saturating_sub(next_position),
    ))
}

pub(crate) fn ceil_minutes(duration: Duration) -> u64 {
    let seconds = duration.as_secs() + u64::from(duration.subsec_nanos() > 0);
    seconds.div_ceil(SECONDS_PER_MINUTE)
}

fn duration_steps(position: Duration, step: Duration) -> u32 {
    let steps = position.as_nanos() / step.as_nanos();
    u32::try_from(steps).unwrap_or(u32::MAX)
}

fn wall_moment(playhead: Playhead, target_position: Duration) -> Moment {
    let delta = target_position.saturating_sub(playhead.offset);
    let wall_delta = delta.div_f32(playhead.speed.get()) + STEP_CORRECTION;
    Moment::new(playhead.started_at.since_epoch() + wall_delta)
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, sync::Arc, time::Duration};

    use kernel::domain::{
        bounded::Bounded,
        geometry::Cells,
        player::{PausedBy, Player},
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track, TrackParts},
    };
    use rstest::rstest;

    use crate::repaint::{
        ProgressScale,
        next_progress_step,
        next_sleep_minute,
        progress_frame_due,
    };

    fn track(duration: Duration) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: "/music/song.mp3".into(),
            duration,
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        }))
    }

    fn playing(offset: Duration, started_at: Moment, speed_factor: f32) -> Player {
        Player::Playing {
            track: track(Duration::from_secs(100)),
            playhead: Playhead::anchored(
                offset,
                started_at,
                Speed::clamped(speed_factor),
            ),
            preloaded: None,
        }
    }

    fn playhead(offset_secs: f64, speed_factor: f32) -> Playhead {
        Playhead::anchored(
            Duration::from_secs_f64(offset_secs),
            Moment::default(),
            Speed::clamped(speed_factor),
        )
    }

    fn hundred_steps() -> ProgressScale {
        ProgressScale {
            steps: NonZeroU32::new(100).unwrap_or(NonZeroU32::MIN),
            duration: Duration::from_secs(100),
        }
    }

    fn paused(position: Duration, duration: Duration) -> Player {
        Player::Paused {
            track: track(duration),
            position,
            by: PausedBy::Listener,
        }
    }

    #[rstest]
    #[case::a_paused_player_has_no_progress_frame(
        paused(Duration::from_secs(10), Duration::from_secs(100)),
        Some(50),
        None
    )]
    #[case::a_playing_track_wants_the_next_progress_step(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            1.0
        ),
        Some(50),
        Some(Moment::new(Duration::from_millis(100_801)))
    )]
    #[case::no_bar_has_no_progress_frame(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            1.0
        ),
        None,
        None
    )]
    #[case::a_zero_length_track_has_no_progress_frame(
        Player::Playing {
            track: track(Duration::ZERO),
            playhead: playhead(0.0, 1.0),
            preloaded: None,
        },
        Some(50),
        None
    )]
    fn a_progress_frame_is_due_only_while_the_bar_can_move(
        #[case] player: Player,
        #[case] bar_width: Option<u16>,
        #[case] expected: Option<Moment>,
    ) {
        let now = Moment::new(Duration::from_secs(100));

        assert_eq!(
            progress_frame_due(&player, bar_width.map(Cells), now),
            expected
        );
    }

    #[rstest]
    #[case::exact_quarter_hour(Duration::from_secs(15 * 60), Duration::from_secs(60))]
    #[case::deadline_itself(Duration::from_secs(30), Duration::from_secs(30))]
    fn the_next_sleep_wake_is_the_nearest_minute_boundary_or_the_deadline(
        #[case] remaining: Duration,
        #[case] until_next: Duration,
    ) {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline_at = Moment::new(now.since_epoch() + remaining);
        assert_eq!(
            next_sleep_minute(deadline_at, now),
            Some(Moment::new(now.since_epoch() + until_next))
        );
    }

    #[test]
    fn next_sleep_minute_is_none_at_or_after_the_deadline() {
        let now = Moment::new(Duration::from_secs(1_000));
        assert_eq!(next_sleep_minute(now, now), None);
        assert_eq!(
            next_sleep_minute(Moment::new(Duration::from_secs(999)), now),
            None
        );
    }

    #[rstest]
    #[case::unity_speed(10.2, 1.0, Some(801))]
    #[case::double_speed(10.2, 2.0, Some(401))]
    #[case::quarter_speed(10.2, 0.25, Some(3201))]
    #[case::exactly_on_a_boundary(10.0, 1.0, Some(1001))]
    #[case::last_step(99.5, 1.0, Some(501))]
    #[case::past_the_end(100.0, 1.0, None)]
    fn next_progress_step_lands_on_the_next_boundary(
        #[case] offset_secs: f64,
        #[case] speed_factor: f32,
        #[case] expected_millis: Option<u64>,
    ) {
        let playhead = playhead(offset_secs, speed_factor);
        let now = Moment::default();
        let result = next_progress_step(hundred_steps(), playhead, now);
        let millis = result.map(|moment| {
            u64::try_from(moment.since_epoch().as_millis()).unwrap_or(u64::MAX)
        });
        assert_eq!(millis, expected_millis);
    }

    #[rstest]
    #[case::zero_width(0, 100, None)]
    #[case::zero_length(40, 0, Some(80))]
    #[case::forty_columns(40, 100, Some(80))]
    fn a_text_bar_has_two_steps_per_column_unless_empty(
        #[case] width: u16,
        #[case] length_secs: u64,
        #[case] expected_steps: Option<u32>,
    ) {
        let scale =
            ProgressScale::text_bar(Cells(width), Duration::from_secs(length_secs));
        assert_eq!(scale.map(|scale| scale.steps.get()), expected_steps);
    }
}
