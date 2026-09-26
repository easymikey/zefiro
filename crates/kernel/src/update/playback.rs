use std::time::Duration;

use crate::{
    cmd::{Cmd, Cue},
    domain::{Model, Moment, Output, SeekSteps},
    message::{PlaybackRequest, SeekTenths},
    update::{
        audio,
        machine::Machine,
        player::{self, Anchor, PlayerMessage, Resume},
        playlist::PlaylistMessage,
        rejection::Rejection,
        transport::TransportMessage,
    },
};

pub(crate) fn playback(
    model: &mut Model,
    message: PlaybackRequest,
    now: Moment,
) -> Result<Cmd, Rejection> {
    match message {
        PlaybackRequest::Toggle => play_pause(model, now),
        PlaybackRequest::Play => resume_playback(model, now),
        PlaybackRequest::Pause => pause_playback(model, now),
        PlaybackRequest::SeekForward => step(model, SeekSteps::default().medium, now),
        PlaybackRequest::SeekBack => step(model, -SeekSteps::default().medium, now),
        PlaybackRequest::Hold => player::account(model, PlayerMessage::Hold(now), now),
        PlaybackRequest::Release => release(model, now),
        PlaybackRequest::Stop => player::account(model, PlayerMessage::Stop, now),
        PlaybackRequest::Next => audio::next(model),
        PlaybackRequest::Prev => audio::previous(model),
        PlaybackRequest::ToggleShuffle => shuffle_toggled(model),
        PlaybackRequest::CycleRepeat => repeat_cycled(model),
        PlaybackRequest::SeekBy(seconds) => step(model, seconds, now),
        PlaybackRequest::NudgeVolume(delta) => volume_nudged(model, delta),
        PlaybackRequest::NudgeSpeed(delta) => speed_nudged(model, delta, now),
        PlaybackRequest::CycleSleep => sleep_cycled(model),
        PlaybackRequest::AbMark => ab_marked(model, now),
        PlaybackRequest::SeekTo(target) => seek_to(model, target, now),
        PlaybackRequest::SeekFraction(tenths) => seek_fraction(model, tenths, now),
    }
}

fn resume_playback(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    if model.player.is_playing() {
        Ok(Cmd::None)
    } else {
        play_pause(model, now)
    }
}

fn pause_playback(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    if model.player.is_playing() {
        play_pause(model, now)
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

fn speed_nudged(model: &mut Model, delta: i8, now: Moment) -> Result<Cmd, Rejection> {
    let cmd = model
        .transport
        .update(TransportMessage::NudgeSpeed(delta))?;
    let since = player::playing_since(&model.player);
    model.player =
        std::mem::take(&mut model.player).reanchored(now, model.transport.speed);
    player::accumulate(&mut model.workspace, since, now);
    Ok(cmd.then(player::arm(model, now, player::PositionReport::Pending)))
}

fn sleep_cycled(model: &mut Model) -> Result<Cmd, Rejection> {
    Ok(model.transport.update(TransportMessage::CycleSleep(
        model.settings.sleep_presets.clone(),
    ))?)
}

fn ab_marked(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    let position = model
        .player
        .current()
        .map(|_| model.player.position_at(now));
    Ok(model
        .transport
        .update(TransportMessage::AbMark { position })?)
}

fn seek_to(model: &mut Model, target: Duration, now: Moment) -> Result<Cmd, Rejection> {
    let target = clamped(model, target);
    seek(model, target, now)
}

fn seek_fraction(
    model: &mut Model,
    tenths: SeekTenths,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let target = fraction_target(model, tenths);
    seek(model, target, now)
}

fn step(model: &mut Model, seconds: i64, now: Moment) -> Result<Cmd, Rejection> {
    let target = relative_target(model, seconds, now);
    seek(model, Some(target), now)
}

pub(crate) fn play_pause(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
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
    let resume = Resume {
        volume: model.transport.volume,
        anchor: Anchor {
            now,
            speed: model.transport.speed,
        },
    };
    player::account(model, PlayerMessage::Toggle { current, resume }, now)
}

fn release(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    let anchor = Anchor {
        now,
        speed: model.transport.speed,
    };
    player::account(model, PlayerMessage::Release(anchor), now)
}

fn seek(
    model: &mut Model,
    target: Option<Duration>,
    now: Moment,
) -> Result<Cmd, Rejection> {
    target.map_or(Ok(Cmd::None), |target| {
        player::account(model, PlayerMessage::Seek { target, now }, now)
    })
}

fn duration_of(model: &Model) -> Duration {
    model
        .player
        .current()
        .map_or(Duration::ZERO, audio::track_duration)
}

fn clamped(model: &Model, target: Duration) -> Option<Duration> {
    let duration = duration_of(model);
    (!duration.is_zero()).then(|| target.min(duration))
}

fn fraction_target(model: &Model, tenths: SeekTenths) -> Option<Duration> {
    let duration = duration_of(model);
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
    moved.min(duration_of(model))
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
        let cmd = playback(
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
