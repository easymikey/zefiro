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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnScreen {
    pub progress_bar: Option<Cells>,
    pub clock: Presence,
    pub sleep_label: Presence,
    pub spectrum: Presence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressScale {
    pub steps: NonZeroU32,
    pub length: Duration,
}

impl ProgressScale {
    #[must_use]
    pub fn text_bar(width: Cells, length: Duration) -> Option<Self> {
        if width == Cells(0) || length.is_zero() {
            return None;
        }
        let steps = NonZeroU32::new(2 * u32::from(width.0))?;
        Some(Self { steps, length })
    }
}

#[must_use]
pub fn next_progress_step(
    scale: ProgressScale,
    playhead: Playhead,
    now: Moment,
) -> Option<Moment> {
    let step_length = scale.length / scale.steps.get();
    if step_length.is_zero() {
        return None;
    }
    let position = playhead.position_at(now);
    let step_index = duration_steps(position, step_length);
    let boundary = step_length.saturating_mul(step_index.saturating_add(1));
    if boundary > scale.length {
        return None;
    }
    Some(wall_moment(playhead, boundary))
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
    let boundary = Duration::from_secs(position.as_secs() + 1);
    wall_moment(playhead, boundary)
}

#[must_use]
pub(crate) fn next_sleep_minute(deadline: Moment, now: Moment) -> Option<Moment> {
    if now >= deadline {
        return None;
    }
    let left = deadline.elapsed_since(now);
    let minutes = ceil_minutes(left);
    let boundary = Duration::from_secs((minutes - 1) * SECONDS_PER_MINUTE);
    Some(Moment::new(deadline.since_epoch().saturating_sub(boundary)))
}

pub(crate) fn ceil_minutes(duration: Duration) -> u64 {
    let seconds = duration.as_secs() + u64::from(duration.subsec_nanos() > 0);
    seconds.div_ceil(SECONDS_PER_MINUTE)
}

fn duration_steps(position: Duration, step_length: Duration) -> u32 {
    let steps = position.as_nanos() / step_length.as_nanos();
    u32::try_from(steps).unwrap_or(u32::MAX)
}

fn wall_moment(playhead: Playhead, target_position: Duration) -> Moment {
    let delta = target_position.saturating_sub(playhead.offset);
    let wall_delta = delta.div_f32(playhead.speed.get()) + STEP_CORRECTION;
    Moment::new(playhead.since.since_epoch() + wall_delta)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        bounded::Bounded,
        geometry::Cells,
        player::{PausedBy, Player},
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track},
    };
    use rstest::rstest;

    use crate::repaint::{next_sleep_minute, progress_frame_due};

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

    fn playing(offset: Duration, since: Moment, speed: f32) -> Player {
        Player::Playing {
            track: track(Duration::from_secs(100)),
            playhead: Playhead::anchored(offset, since, Speed::clamped(speed)),
            preloaded: None,
        }
    }

    fn paused(at: Duration, duration: Duration) -> Player {
        Player::Paused {
            track: track(duration),
            position: at,
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
    #[case::a_sped_up_track_still_wants_a_progress_step(
        playing(
            Duration::from_millis(10_500),
            Moment::new(Duration::from_secs(100)),
            2.0
        ),
        Some(50),
        Some(Moment::new(Duration::from_millis(100_251)))
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
    #[case::minute_boundary_soon(Duration::from_secs(14 * 60 + 59), Duration::from_secs(59))]
    #[case::exact_quarter_hour(Duration::from_secs(15 * 60), Duration::from_secs(60))]
    #[case::deadline_itself(Duration::from_secs(30), Duration::from_secs(30))]
    fn the_next_sleep_wake_is_the_nearest_minute_boundary_or_the_deadline(
        #[case] left: Duration,
        #[case] until_next: Duration,
    ) {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline = Moment::new(now.since_epoch() + left);
        assert_eq!(
            next_sleep_minute(deadline, now),
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
}
