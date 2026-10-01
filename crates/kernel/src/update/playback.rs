use std::time::Duration;

use crate::{
    cmd::{Cmd, Cue},
    domain::{Model, Moment, Output, SEEK_MEDIUM},
    message::{PlaybackRequest, SeekTenths},
    update::{
        audio,
        error::UpdateError,
        machine::Machine,
        player::{self, Anchor, PlayerMessage, Stamp},
        playlist::PlaylistMessage,
        transport::TransportMessage,
    },
};

pub(crate) fn update(
    model: &mut Model,
    message: PlaybackRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match message {
        PlaybackRequest::Toggle => play_pause(model, now),
        PlaybackRequest::Play => resume_playback(model, now),
        PlaybackRequest::Pause => pause_playback(model, now),
        PlaybackRequest::SeekForward => seek_by(model, SEEK_MEDIUM, now),
        PlaybackRequest::SeekBack => seek_by(model, -SEEK_MEDIUM, now),
        PlaybackRequest::Hold => {
            player::update_player(model, PlayerMessage::Hold(now), now)
        }
        PlaybackRequest::Release => release(model, now),
        PlaybackRequest::Stop => player::update_player(model, PlayerMessage::Stop, now),
        PlaybackRequest::Next => audio::next(model, now),
        PlaybackRequest::Previous => audio::previous(model, now),
        PlaybackRequest::ToggleShuffle => toggle_shuffle(model),
        PlaybackRequest::CycleRepeat => cycle_repeat(model),
        PlaybackRequest::SeekBy { seconds } => seek_by(model, seconds, now),
        PlaybackRequest::NudgeVolume { steps } => nudge_volume(model, steps),
        PlaybackRequest::NudgeSpeed { steps } => nudge_speed(model, steps, now),
        PlaybackRequest::CycleSleep => cycle_sleep(model),
        PlaybackRequest::AbMark => mark_ab(model, now),
        PlaybackRequest::SeekTo(target) => seek_to(model, target, now),
        PlaybackRequest::SeekFraction(tenths) => seek_fraction(model, tenths, now),
    }
}

fn resume_playback(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    if model.player.is_playing() {
        Ok(Cmd::None)
    } else {
        play_pause(model, now)
    }
}

fn pause_playback(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    if model.player.is_playing() {
        play_pause(model, now)
    } else {
        Ok(Cmd::None)
    }
}

fn toggle_shuffle(model: &mut Model) -> Result<Cmd, UpdateError> {
    let cmd = model.playlist.update(PlaylistMessage::ToggleShuffle)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn cycle_repeat(model: &mut Model) -> Result<Cmd, UpdateError> {
    let cmd = model.playlist.update(PlaylistMessage::CycleRepeat)?;
    Ok(cmd.then(Cue::PlayOrderChanged.into()))
}

fn nudge_volume(model: &mut Model, steps: i8) -> Result<Cmd, UpdateError> {
    let cmd = model
        .transport
        .update(TransportMessage::StepVolume { steps })?;
    Ok(cmd.then(Cue::VolumeChanged.into()))
}

fn nudge_speed(model: &mut Model, steps: i8, now: Moment) -> Result<Cmd, UpdateError> {
    let cmd = model
        .transport
        .update(TransportMessage::StepSpeed { steps })?;
    let since = player::playing_since(&model.player);
    model.player =
        std::mem::take(&mut model.player).reanchored(now, model.transport.speed);
    if let Some(since) = since {
        player::accumulate(&mut model.workspace, since, now);
    }
    Ok(cmd.then(player::arm(model, now)))
}

fn cycle_sleep(model: &mut Model) -> Result<Cmd, UpdateError> {
    let candidate = model.revisions.effects.next();
    let cmd = model.transport.update(TransportMessage::CycleSleep {
        presets: model.settings.audio.sleep_presets.clone(),
        revision: candidate,
    })?;
    if model.transport.sleep.is_some() {
        model.revisions.commit_sleep(candidate);
    }
    Ok(cmd)
}

fn mark_ab(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let position = model
        .player
        .current()
        .map(|_| model.player.position_at(now));
    Ok(model
        .transport
        .update(TransportMessage::AbMark { position })?)
}

fn seek_to(
    model: &mut Model,
    target: Duration,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    clamped(model, target)
        .map_or_else(|| Ok(Cmd::None), |target| seek(model, target, now))
}

fn seek_fraction(
    model: &mut Model,
    tenths: SeekTenths,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    fraction_target(model, tenths)
        .map_or_else(|| Ok(Cmd::None), |target| seek(model, target, now))
}

fn seek_by(model: &mut Model, seconds: i64, now: Moment) -> Result<Cmd, UpdateError> {
    let target = relative_target(model, seconds, now);
    seek(model, target, now)
}

pub(crate) fn play_pause(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    if matches!(model.transport.output, Output::Lost { .. })
        && !model.player.is_playing()
    {
        let again = model
            .player
            .current()
            .cloned()
            .or_else(|| model.playlist.current().cloned());
        return again.map_or(Ok(Cmd::None), |track| audio::start(model, track, now));
    }
    let current = model.playlist.current().cloned();
    let stamp = Stamp::issue(model, now);
    player::update_player(model, PlayerMessage::Toggle { current, stamp }, now)
}

fn release(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let anchor = Anchor::at(model, now);
    player::update_player(model, PlayerMessage::Release(anchor), now)
}

fn seek(model: &mut Model, target: Duration, now: Moment) -> Result<Cmd, UpdateError> {
    player::update_player(model, PlayerMessage::Seek { target, now }, now)
}

fn clamped(model: &Model, target: Duration) -> Option<Duration> {
    let duration = player::duration_of(model);
    (!duration.is_zero()).then(|| target.min(duration))
}

fn fraction_target(model: &Model, tenths: SeekTenths) -> Option<Duration> {
    let duration = player::duration_of(model);
    if duration.is_zero() {
        return None;
    }
    clamped(model, duration * u32::from(tenths.tenths()) / 10)
}

fn relative_target(model: &Model, seconds: i64, now: Moment) -> Duration {
    let at = model.player.position_at(now);
    let moved = if seconds < 0 {
        at.saturating_sub(Duration::from_secs(seconds.unsigned_abs()))
    } else {
        at + Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
    };
    moved.min(player::duration_of(model))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect},
        domain::{
            AudioFormat,
            Model,
            Moment,
            Player,
            Playhead,
            Preload,
            Speed,
            Tags,
            Track,
        },
        message::{PlaybackRequest, SeekTenths},
        update::playback::update,
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
        let cmd = update(
            &mut model,
            PlaybackRequest::SeekFraction(tenths),
            Moment::default(),
        )
        .unwrap();
        case.expected.map_or_else(
            || {
                assert!(matches!(cmd, Cmd::None));
                assert_eq!(model.player.position_at(Moment::default()), case.at);
            },
            |target| {
                assert!(seeks_to(&cmd, target));
                assert_eq!(model.player.position_at(Moment::default()), target);
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
