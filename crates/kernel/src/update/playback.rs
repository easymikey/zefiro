use std::time::Duration;

use crate::{
    cmd::Cmd,
    domain::{
        cue::Cue,
        direction::Direction,
        player::Player,
        time::Moment,
        transport::{Output, SEEK_MEDIUM},
    },
    message::{PlaybackRequest, SeekTenths},
    update::{
        audio,
        machine::{Machine, Unhandled},
        player,
        player::{
            PlaybackParts,
            PlayerMessage,
            stamp::{Anchor, Stamp},
        },
        playlist::PlaylistMessage,
        transport::TransportMessage,
    },
};

pub(crate) fn update(
    playback: &mut PlaybackParts<'_>,
    message: PlaybackRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match message {
        PlaybackRequest::Toggle => play_pause(playback, now),
        PlaybackRequest::Play => resume_playback(playback, now),
        PlaybackRequest::Pause => pause_playback(playback, now),
        PlaybackRequest::SeekForward => {
            seek_by(playback, (Direction::Next, SEEK_MEDIUM), now)
        }
        PlaybackRequest::SeekBack => {
            seek_by(playback, (Direction::Previous, SEEK_MEDIUM), now)
        }
        PlaybackRequest::HoldForOverlay => {
            player::update_player(playback, PlayerMessage::Hold(now), now)
        }
        PlaybackRequest::Release => release(playback, now),
        PlaybackRequest::Stop => {
            player::update_player(playback, PlayerMessage::Stop, now)
        }
        PlaybackRequest::Next => audio::next(playback, now),
        PlaybackRequest::Previous => audio::previous(playback, now),
        PlaybackRequest::ToggleShuffle => toggle_shuffle(playback),
        PlaybackRequest::CycleRepeat => cycle_repeat(playback),
        PlaybackRequest::SeekBy { direction, by } => {
            seek_by(playback, (direction, by), now)
        }
        PlaybackRequest::StepVolume(direction) => step_volume(playback, direction),
        PlaybackRequest::StepSpeed(direction) => step_speed(playback, direction, now),
        PlaybackRequest::CycleSleep => cycle_sleep(playback, now),
        PlaybackRequest::AbMark => mark_ab(playback, now),
        PlaybackRequest::SeekTo(target) => seek_to(playback, target, now),
        PlaybackRequest::SeekTenths(tenths) => seek_tenths(playback, tenths, now),
    }
}

fn resume_playback(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if playback.player.is_playing() {
        Err(Unhandled)
    } else {
        play_pause(playback, now)
    }
}

fn pause_playback(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if playback.player.is_playing() {
        play_pause(playback, now)
    } else {
        Err(Unhandled)
    }
}

fn toggle_shuffle(playback: &mut PlaybackParts<'_>) -> Result<Cmd, Unhandled> {
    let cmd = playback
        .playlist
        .transition(PlaylistMessage::ToggleShuffle)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn cycle_repeat(playback: &mut PlaybackParts<'_>) -> Result<Cmd, Unhandled> {
    let cmd = playback.playlist.transition(PlaylistMessage::CycleRepeat)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn step_volume(
    playback: &mut PlaybackParts<'_>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let cmd = playback
        .transport
        .transition(TransportMessage::StepVolume(direction))?;
    Ok(cmd.then(Cue::VolumeChanged.into()))
}

fn step_speed(
    playback: &mut PlaybackParts<'_>,
    direction: Direction,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let cmd = playback
        .transport
        .transition(TransportMessage::StepSpeed(direction))?;
    let since = player::playing_since(playback.player);
    *playback.player = std::mem::replace(playback.player, Player::Stopped)
        .reanchored(now, playback.transport.speed);
    if let Some(since) = since {
        player::accumulate(playback.workspace, since, now);
    }
    Ok(cmd.then(player::arm(playback, now)))
}

fn cycle_sleep(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let candidate = playback.revisions.effects.next();
    let cmd = playback
        .transport
        .transition(TransportMessage::CycleSleep {
            presets: playback.settings.audio.sleep_presets.clone(),
            revision: candidate,
            now,
        })?;
    if playback.transport.sleep.is_some() {
        playback.revisions.commit_sleep(candidate);
    }
    Ok(cmd)
}

fn mark_ab(playback: &mut PlaybackParts<'_>, now: Moment) -> Result<Cmd, Unhandled> {
    let position = playback
        .player
        .current()
        .map(|_| playback.player.position_at(now));
    playback
        .transport
        .transition(TransportMessage::AbMark(position))
}

fn seek_to(
    playback: &mut PlaybackParts<'_>,
    target: Duration,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    clamped(playback.player, target)
        .map_or_else(|| Ok(Cmd::none()), |target| seek(playback, target, now))
}

fn seek_tenths(
    playback: &mut PlaybackParts<'_>,
    tenths: SeekTenths,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    tenths_target(playback.player, tenths)
        .map_or_else(|| Ok(Cmd::none()), |target| seek(playback, target, now))
}

fn seek_by(
    playback: &mut PlaybackParts<'_>,
    step: (Direction, Duration),
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let target = relative_target(playback.player, step, now);
    seek(playback, target, now)
}

pub(crate) fn play_pause(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if matches!(playback.transport.output, Output::Lost(..))
        && !playback.player.is_playing()
    {
        let again = playback
            .player
            .current()
            .cloned()
            .or_else(|| playback.playlist.current().cloned());
        return again
            .map_or(Ok(Cmd::none()), |track| audio::start(playback, track, now));
    }
    let current = playback.playlist.current().cloned();
    let stamp = Stamp::pending(playback.transport, playback.revisions, now);
    player::update_player(playback, PlayerMessage::Toggle { current, stamp }, now)
}

fn release(playback: &mut PlaybackParts<'_>, now: Moment) -> Result<Cmd, Unhandled> {
    let anchor = Anchor::at(playback.transport, now);
    player::update_player(playback, PlayerMessage::Release(anchor), now)
}

fn seek(
    playback: &mut PlaybackParts<'_>,
    target: Duration,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    player::update_player(playback, PlayerMessage::Seek { target, now }, now)
}

fn clamped(player: &Player, target: Duration) -> Option<Duration> {
    let duration = player::duration_of(player);
    (!duration.is_zero()).then(|| target.min(duration))
}

fn tenths_target(player: &Player, tenths: SeekTenths) -> Option<Duration> {
    let duration = player::duration_of(player);
    if duration.is_zero() {
        return None;
    }
    clamped(player, duration * u32::from(tenths.get()) / 10)
}

fn relative_target(
    player: &Player,
    (direction, by): (Direction, Duration),
    now: Moment,
) -> Duration {
    let at = player.position_at(now);
    let moved = match direction {
        Direction::Previous => at.saturating_sub(by),
        Direction::Next => at + by,
    };
    clamped(player, moved).unwrap_or(moved)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect},
        domain::{
            model::Model,
            player::{PausedBy, Player, Preload},
            playhead::Playhead,
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track},
            transport::SEEK_MEDIUM,
        },
        message::{PlaybackRequest, SeekTenths},
        update::{machine::Unhandled, playback::update, playback_parts},
    };

    fn playing_track_at(duration: Option<Duration>, at: Duration) -> Model {
        let track = Arc::new(duration.map_or_else(
            || Track::listed(Path::new("/t.flac")),
            |duration| {
                Track::builder()
                    .path("/t.flac")
                    .duration(duration)
                    .tags(Tags::default())
                    .audio_format(AudioFormat::default())
                    .build()
            },
        ));
        Model {
            player: Player::Playing {
                track,
                head: Playhead::anchored(at, Moment::default(), Speed::default()),
                preload: Preload::None,
            },
            ..Model::default()
        }
    }

    struct SeekTenthsRow {
        duration: Option<Duration>,
        at: Duration,
        tenths: u8,
        expected: Option<Duration>,
    }

    #[rstest]
    #[case::digit_five_seeks_to_fifty_percent(SeekTenthsRow {
        duration: Some(Duration::from_secs(200)),
        at: Duration::ZERO,
        tenths: 5,
        expected: Some(Duration::from_secs(100)),
    })]
    #[case::digit_zero_seeks_to_the_start(SeekTenthsRow {
        duration: Some(Duration::from_secs(200)),
        at: Duration::from_secs(150),
        tenths: 0,
        expected: Some(Duration::ZERO),
    })]
    #[case::unknown_duration_is_a_no_op(SeekTenthsRow {
        duration: None,
        at: Duration::ZERO,
        tenths: 7,
        expected: None,
    })]
    fn seek_tenths_seeks_by_tenths(#[case] case: SeekTenthsRow) {
        let mut model = playing_track_at(case.duration, case.at);
        let tenths = SeekTenths::try_from(case.tenths).unwrap();
        let cmd = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::SeekTenths(tenths),
            Moment::default(),
        )
        .unwrap();
        case.expected.map_or_else(
            || {
                assert!(cmd == Cmd::none());
                assert_eq!(model.player.position_at(Moment::default()), case.at);
            },
            |target| {
                assert!(seeks_to(&cmd, target));
                assert_eq!(model.player.position_at(Moment::default()), target);
            },
        );
    }

    #[test]
    fn seeking_forward_with_an_unknown_duration_moves_past_the_position() {
        let at = Duration::from_secs(30);
        let mut model = playing_track_at(None, at);

        let cmd = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::SeekForward,
            Moment::default(),
        )
        .unwrap();

        assert!(seeks_to(&cmd, at + SEEK_MEDIUM));
    }

    #[rstest]
    #[case::play_while_playing(
        playing_track_at(None, Duration::ZERO),
        PlaybackRequest::Play
    )]
    #[case::pause_while_paused(paused(), PlaybackRequest::Pause)]
    fn a_request_for_the_current_state_is_refused(
        #[case] mut model: Model,
        #[case] request: PlaybackRequest,
    ) {
        let refused =
            update(&mut playback_parts(&mut model), request, Moment::default());

        assert_eq!(refused, Err(Unhandled));
    }

    fn paused() -> Model {
        Model {
            player: Player::Paused {
                track: Arc::new(Track::listed(Path::new("/t.flac"))),
                at: Duration::ZERO,
                by: PausedBy::Listener,
            },
            ..Model::default()
        }
    }

    fn seeks_to(cmd: &Cmd, target: Duration) -> bool {
        matches!(
            cmd.effects().as_slice(),
            [Effect::Audio(AudioCmd::Seek(at)), ..] if *at == target
        )
    }
}
