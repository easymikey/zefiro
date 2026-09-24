use std::time::Duration;

use crate::{
    cmd::{Cmd, Cue},
    domain::{Model, Output, Player, SeekSteps},
    message::{PlaybackRequest, SeekTenths},
    update::{
        audio,
        machine::Machine,
        player::PlayerMessage,
        playlist::PlaylistMessage,
        rejection::Rejection,
        transport::TransportMessage,
    },
};

pub(super) fn playback(
    model: &mut Model,
    message: PlaybackRequest,
) -> Result<Cmd, Rejection> {
    match message {
        PlaybackRequest::Toggle => play_pause(model),
        PlaybackRequest::Play => resume_playback(model),
        PlaybackRequest::Pause => pause_playback(model),
        PlaybackRequest::SeekForward => {
            step(&mut model.player, SeekSteps::default().medium)
        }
        PlaybackRequest::SeekBack => {
            step(&mut model.player, -SeekSteps::default().medium)
        }
        PlaybackRequest::Hold => Ok(model.player.update(PlayerMessage::Hold)?),
        PlaybackRequest::Release => Ok(model.player.update(PlayerMessage::Release)?),
        PlaybackRequest::Stop => Ok(model.player.update(PlayerMessage::Stop)?),
        PlaybackRequest::Next => audio::next(model),
        PlaybackRequest::Prev => audio::previous(model),
        PlaybackRequest::ToggleShuffle => shuffle_toggled(model),
        PlaybackRequest::CycleRepeat => repeat_cycled(model),
        PlaybackRequest::SeekBy(seconds) => step(&mut model.player, seconds),
        PlaybackRequest::NudgeVolume(delta) => volume_nudged(model, delta),
        PlaybackRequest::NudgeSpeed(delta) => Ok(model
            .transport
            .update(TransportMessage::NudgeSpeed(delta))?),
        PlaybackRequest::CycleSleep => sleep_cycled(model),
        PlaybackRequest::AbMark => ab_marked(model),
        PlaybackRequest::SeekTo(target) => seek_to(&mut model.player, target),
        PlaybackRequest::SeekFraction(tenths) => {
            seek_fraction(&mut model.player, tenths)
        }
    }
}

fn resume_playback(model: &mut Model) -> Result<Cmd, Rejection> {
    if model.player.is_playing() {
        Ok(Cmd::None)
    } else {
        play_pause(model)
    }
}

fn pause_playback(model: &mut Model) -> Result<Cmd, Rejection> {
    if model.player.is_playing() {
        play_pause(model)
    } else {
        Ok(Cmd::None)
    }
}

fn shuffle_toggled(model: &mut Model) -> Result<Cmd, Rejection> {
    let cmd = model.playlist.update(PlaylistMessage::ToggleShuffle)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn repeat_cycled(model: &mut Model) -> Result<Cmd, Rejection> {
    let cmd = model.playlist.update(PlaylistMessage::CycleRepeat)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn volume_nudged(model: &mut Model, delta: i8) -> Result<Cmd, Rejection> {
    let cmd = model
        .transport
        .update(TransportMessage::NudgeVolume(delta))?;
    Ok(cmd.then(Cue::VolumeChanged.into()))
}

fn sleep_cycled(model: &mut Model) -> Result<Cmd, Rejection> {
    Ok(model.transport.update(TransportMessage::CycleSleep(
        model.settings.sleep_presets.clone(),
    ))?)
}

fn ab_marked(model: &mut Model) -> Result<Cmd, Rejection> {
    let position = model.player.current().map(|_| model.player.position());
    Ok(model
        .transport
        .update(TransportMessage::AbMark { position })?)
}

fn seek_to(player: &mut Player, target: Duration) -> Result<Cmd, Rejection> {
    let target = clamped(player, target);
    seek(player, target)
}

fn seek_fraction(player: &mut Player, tenths: SeekTenths) -> Result<Cmd, Rejection> {
    let target = fraction_target(player, tenths);
    seek(player, target)
}

fn step(player: &mut Player, seconds: i64) -> Result<Cmd, Rejection> {
    let target = relative_target(player, seconds);
    seek(player, Some(target))
}

pub(super) fn play_pause(model: &mut Model) -> Result<Cmd, Rejection> {
    if matches!(model.transport.output, Output::Lost { .. })
        && !model.player.is_playing()
    {
        let again = model
            .player
            .current()
            .cloned()
            .or_else(|| model.playlist.current().cloned());
        return again.map_or(Ok(Cmd::None), |track| {
            audio::start(&mut model.transport, &mut model.player, track)
        });
    }
    let current = model.playlist.current().cloned();
    Ok(model.player.update(PlayerMessage::Toggle {
        current,
        volume: model.transport.volume,
    })?)
}

fn seek(player: &mut Player, target: Option<Duration>) -> Result<Cmd, Rejection> {
    target.map_or(Ok(Cmd::None), |target| {
        Ok(player.update(PlayerMessage::Seek(target))?)
    })
}

fn duration_of(player: &Player) -> Duration {
    player
        .current()
        .map_or(Duration::ZERO, audio::track_duration)
}

fn clamped(player: &Player, target: Duration) -> Option<Duration> {
    let duration = duration_of(player);
    (!duration.is_zero()).then(|| target.min(duration))
}

fn fraction_target(player: &Player, tenths: SeekTenths) -> Option<Duration> {
    let duration = duration_of(player);
    if duration.is_zero() {
        return None;
    }
    clamped(player, duration * u32::from(tenths.tenths()) / 10)
}

fn relative_target(player: &Player, seconds: i64) -> Duration {
    let at = player.position();
    let moved = if seconds < 0 {
        at.saturating_sub(Duration::from_secs(seconds.unsigned_abs()))
    } else {
        at + Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
    };
    moved.min(duration_of(player))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect},
        domain::{AudioFormat, Model, Player, Preload, Tags, Track},
        message::{PlaybackRequest, SeekTenths},
        update::playback::playback,
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
                at,
                preload: Preload::None,
            },
            ..Model::default()
        }
    }

    struct SeekFractionCase {
        duration: Option<Duration>,
        at: Duration,
        tenths: u8,
        expected: Option<Duration>,
    }

    #[rstest]
    #[case::digit_five_seeks_to_fifty_percent(SeekFractionCase {
        duration: Some(Duration::from_secs(200)),
        at: Duration::ZERO,
        tenths: 5,
        expected: Some(Duration::from_secs(100)),
    })]
    #[case::digit_zero_seeks_to_the_start(SeekFractionCase {
        duration: Some(Duration::from_secs(200)),
        at: Duration::from_secs(150),
        tenths: 0,
        expected: Some(Duration::ZERO),
    })]
    #[case::unknown_duration_is_a_no_op(SeekFractionCase {
        duration: None,
        at: Duration::ZERO,
        tenths: 7,
        expected: None,
    })]
    fn seek_fraction_seeks_by_tenths(#[case] case: SeekFractionCase) {
        let mut model = playing_track_at(case.duration, case.at);
        let tenths = SeekTenths::try_from(case.tenths).unwrap();
        let cmd = playback(&mut model, PlaybackRequest::SeekFraction(tenths)).unwrap();
        case.expected.map_or_else(
            || {
                assert!(matches!(cmd, Cmd::None));
                assert_eq!(model.player.position(), case.at);
            },
            |target| {
                assert!(seeks_to(&cmd, target));
                assert_eq!(model.player.position(), target);
            },
        );
    }

    fn seeks_to(cmd: &Cmd, target: Duration) -> bool {
        matches!(
            cmd,
            Cmd::Batch(effects)
                if matches!(effects.first(), Some(Effect::Audio(AudioCmd::Seek(at))) if *at == target)
        )
    }
}
