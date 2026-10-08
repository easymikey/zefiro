use std::time::Duration;

use crate::{
    cmd::Cmd,
    domain::{
        cue::Cue,
        direction::Direction,
        player::{PausedBy, Player},
        playlist::Playlist,
        time::Moment,
        transport::OutputStatus,
    },
    message::{PlaybackRequest, SeekTenths},
    update::{
        audio,
        machine::{Machine, Unhandled},
        player,
        player::{
            PlayerMessage,
            events::PlaybackParts,
            stamp::{Anchor, Stamp},
        },
        playlist::PlaylistMessage,
        transport::{TransportMessage, next_sleep},
    },
};

pub(crate) fn update(
    playback_parts: &mut PlaybackParts<'_>,
    request: PlaybackRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match request {
        PlaybackRequest::Toggle => play_pause(playback_parts, now),
        PlaybackRequest::Play => resume_playback(playback_parts, now),
        PlaybackRequest::Pause => pause_playback(playback_parts, now),
        PlaybackRequest::HoldForOverlay => {
            player::update_player(playback_parts, PlayerMessage::Hold(now), now)
        }
        PlaybackRequest::Release => release(playback_parts, now),
        PlaybackRequest::Stop => {
            player::update_player(playback_parts, PlayerMessage::Stop, now)
        }
        PlaybackRequest::Next => audio::next(playback_parts, now),
        PlaybackRequest::Previous => audio::previous(playback_parts, now),
        PlaybackRequest::ToggleShuffle => {
            reorder(playback_parts.playlist, PlaylistMessage::ToggleShuffle)
        }
        PlaybackRequest::CycleRepeat => {
            reorder(playback_parts.playlist, PlaylistMessage::CycleRepeat)
        }
        PlaybackRequest::SeekBy { direction, by } => {
            seek_by(playback_parts, SeekStep { direction, by }, now)
        }
        PlaybackRequest::StepVolume(direction) => {
            step_volume(playback_parts, direction)
        }
        PlaybackRequest::StepSpeed(direction) => {
            step_speed(playback_parts, direction, now)
        }
        PlaybackRequest::CycleSleep => cycle_sleep(playback_parts, now),
        PlaybackRequest::AbMark => mark_ab(playback_parts, now),
        PlaybackRequest::SeekTo(target) => seek_to(playback_parts, target, now),
        PlaybackRequest::SeekTenths(tenths) => seek_tenths(playback_parts, tenths, now),
        PlaybackRequest::JumpTo(index) => audio::jump_to(playback_parts, index, now),
    }
}

fn resume_playback(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if playback_parts.player.is_playing() {
        Err(Unhandled)
    } else {
        play_pause(playback_parts, now)
    }
}

fn pause_playback(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if playback_parts.player.is_playing() {
        play_pause(playback_parts, now)
    } else {
        Err(Unhandled)
    }
}

fn reorder(
    playlist: &mut Playlist,
    message: PlaylistMessage,
) -> Result<Cmd, Unhandled> {
    Ok(playlist
        .transition(message)?
        .then(Cue::PlayOrderChanged.into()))
}

fn step_volume(
    playback_parts: &mut PlaybackParts<'_>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let cmd = playback_parts
        .transport
        .transition(TransportMessage::StepVolume(direction))?;
    Ok(cmd.then(Cue::VolumeChanged.into()))
}

fn step_speed(
    playback_parts: &mut PlaybackParts<'_>,
    direction: Direction,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let stepped = playback_parts
        .transport
        .transition(TransportMessage::StepSpeed(direction))?;
    let anchor = Anchor::at(playback_parts.transport, now);
    match playback_parts
        .player
        .transition(PlayerMessage::SpeedChanged(anchor))
    {
        Ok(reanchored) => Ok(stepped
            .then(reanchored)
            .then(player::arm(playback_parts, now))),
        Err(Unhandled) => Ok(stepped),
    }
}

fn cycle_sleep(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let candidate = playback_parts.revisions.effects.next();
    let sleep_timer = next_sleep(
        playback_parts.transport.sleep_timer,
        playback_parts
            .settings
            .audio_settings
            .sleep_presets
            .as_slice(),
        now,
    );
    let cmd = playback_parts
        .transport
        .transition(TransportMessage::CycleSleep {
            sleep_timer,
            revision: candidate,
        })?;
    playback_parts.revisions.effects = candidate;
    playback_parts.revisions.sleep = candidate;
    Ok(cmd)
}

fn mark_ab(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let position = playback_parts
        .player
        .current()
        .map(|_| playback_parts.player.position_at(now));
    playback_parts
        .transport
        .transition(TransportMessage::AbMark(position))
}

fn seek_to(
    playback_parts: &mut PlaybackParts<'_>,
    target: Duration,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let target = clamped(playback_parts.player, target).ok_or(Unhandled)?;
    seek(playback_parts, target, now)
}

fn seek_tenths(
    playback_parts: &mut PlaybackParts<'_>,
    tenths: SeekTenths,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let target = tenths_target(playback_parts.player, tenths).ok_or(Unhandled)?;
    seek(playback_parts, target, now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeekStep {
    direction: Direction,
    by: Duration,
}

fn seek_by(
    playback_parts: &mut PlaybackParts<'_>,
    seek_step: SeekStep,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let target = relative_target(playback_parts.player, seek_step, now);
    seek(playback_parts, target, now)
}

pub(crate) fn play_pause(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if lost(playback_parts) && !playback_parts.player.is_playing() {
        return restart(playback_parts, now);
    }
    let current = playback_parts.playlist.current().cloned();
    let stamp = Stamp::pending(playback_parts.transport, playback_parts.revisions, now);
    player::update_player(
        playback_parts,
        PlayerMessage::Toggle { current, stamp },
        now,
    )
}

fn release(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let held = matches!(
        playback_parts.player,
        Player::Paused {
            by: PausedBy::Overlay,
            track: _track,
            position: _position
        }
    );
    if lost(playback_parts) && held {
        return restart(playback_parts, now);
    }
    let anchor = Anchor::at(playback_parts.transport, now);
    player::update_player(playback_parts, PlayerMessage::Release(anchor), now)
}

fn lost(playback_parts: &PlaybackParts<'_>) -> bool {
    matches!(
        playback_parts.transport.output_status,
        OutputStatus::Lost(..)
    )
}

fn restart(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let again = playback_parts
        .player
        .current()
        .cloned()
        .or_else(|| playback_parts.playlist.current().cloned());
    let track = again.ok_or(Unhandled)?;
    Ok(player::start(playback_parts, track, now)
        .unwrap_or_else(|offline_toast| offline_toast))
}

fn seek(
    playback_parts: &mut PlaybackParts<'_>,
    target: Duration,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    player::update_player(playback_parts, PlayerMessage::Seek { target, now }, now)
}

fn clamped(player: &Player, target: Duration) -> Option<Duration> {
    let duration = player::events::duration_of(player);
    (!duration.is_zero()).then(|| target.min(duration))
}

fn tenths_target(player: &Player, tenths: SeekTenths) -> Option<Duration> {
    let duration = player::events::duration_of(player);
    (!duration.is_zero()).then(|| duration * u32::from(tenths.get()) / 10)
}

fn relative_target(player: &Player, seek_step: SeekStep, now: Moment) -> Duration {
    let SeekStep { direction, by } = seek_step;
    let position = player.position_at(now);
    let moved = match direction {
        Direction::Previous => position.saturating_sub(by),
        Direction::Next => position + by,
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
            bounded::Bounded,
            direction::Direction,
            model::Model,
            player::{PausedBy, Player},
            playhead::Playhead,
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track, TrackParts},
            transport::SEEK_MEDIUM,
        },
        message::{PlaybackRequest, SeekTenths},
        update::{machine::Unhandled, playback::update, playback_parts},
    };

    fn playing_track_at(duration: Option<Duration>, position: Duration) -> Model {
        let track = Arc::new(duration.map_or_else(
            || Track::listed(Path::new("/t.flac")),
            |duration| {
                Track::new(TrackParts {
                    path: "/t.flac".into(),
                    duration,
                    tags: Tags::default(),
                    audio_format: AudioFormat::default(),
                })
            },
        ));
        Model {
            player: Player::Playing {
                track,
                playhead: Playhead::anchored(
                    position,
                    Moment::default(),
                    Speed::default(),
                ),
                preloaded: None,
            },
            ..Model::default()
        }
    }

    struct SeekTenthsRow {
        duration: Option<Duration>,
        position: Duration,
        tenths: u8,
        expected: Option<Duration>,
    }

    #[rstest]
    #[case::digit_five_seeks_to_fifty_percent(SeekTenthsRow {
        duration: Some(Duration::from_secs(200)),
        position: Duration::ZERO,
        tenths: 5,
        expected: Some(Duration::from_secs(100)),
    })]
    #[case::digit_zero_seeks_to_the_start(SeekTenthsRow {
        duration: Some(Duration::from_secs(200)),
        position: Duration::from_secs(150),
        tenths: 0,
        expected: Some(Duration::ZERO),
    })]
    #[case::unknown_duration_is_refused(SeekTenthsRow {
        duration: None,
        position: Duration::ZERO,
        tenths: 7,
        expected: None,
    })]
    fn seek_tenths_seeks_by_tenths(#[case] seek_tenths_row: SeekTenthsRow) {
        let mut model =
            playing_track_at(seek_tenths_row.duration, seek_tenths_row.position);
        let tenths = SeekTenths::clamped(seek_tenths_row.tenths);
        let result = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::SeekTenths(tenths),
            Moment::default(),
        );
        let Some(target) = seek_tenths_row.expected else {
            assert_eq!(result, Err(Unhandled));
            assert_eq!(
                model.player.position_at(Moment::default()),
                seek_tenths_row.position
            );
            return;
        };
        assert!(seeks_to(&result.unwrap(), target));
        assert_eq!(model.player.position_at(Moment::default()), target);
    }

    #[test]
    fn seeking_to_a_time_with_an_unknown_duration_is_refused() {
        let duration = Duration::from_secs(30);
        let mut model = playing_track_at(None, duration);

        let result = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::SeekTo(Duration::from_secs(90)),
            Moment::default(),
        );

        assert_eq!(result, Err(Unhandled));
        assert_eq!(model.player.position_at(Moment::default()), duration);
    }

    #[test]
    fn seeking_forward_with_an_unknown_duration_moves_past_the_position() {
        let position = Duration::from_secs(30);
        let mut model = playing_track_at(None, position);

        let cmd = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::SeekBy {
                direction: Direction::Next,
                by: SEEK_MEDIUM,
            },
            Moment::default(),
        )
        .unwrap();

        assert!(seeks_to(&cmd, position + SEEK_MEDIUM));
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
        let result =
            update(&mut playback_parts(&mut model), request, Moment::default());

        assert_eq!(result, Err(Unhandled));
    }

    #[test]
    fn a_speed_step_while_paused_changes_the_speed_and_keeps_the_pause() {
        let mut model = paused();
        let before = model.player.clone();

        let result = update(
            &mut playback_parts(&mut model),
            PlaybackRequest::StepSpeed(Direction::Next),
            Moment::default(),
        );

        assert!(result.is_ok());
        assert_ne!(model.transport.speed, Speed::default());
        assert_eq!(model.player, before);
    }

    fn paused() -> Model {
        Model {
            player: Player::Paused {
                track: Arc::new(Track::listed(Path::new("/t.flac"))),
                position: Duration::ZERO,
                by: PausedBy::Listener,
            },
            ..Model::default()
        }
    }

    fn seeks_to(cmd: &Cmd, expected: Duration) -> bool {
        matches!(
            cmd.effects().as_slice(),
            [Effect::Audio(AudioCmd::Seek(target)), ..] if *target == expected
        )
    }
}
