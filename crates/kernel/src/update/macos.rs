use crate::{
    cmd::Cmd,
    domain::{Moment, Toast},
    message::{MacosEvent, PlaybackRequest},
    update::{
        error::UpdateError,
        playback,
        player::PlaybackParts,
        transport::TransportMessage,
    },
};

pub(crate) fn update(
    playback: &mut PlaybackParts<'_>,
    event: MacosEvent,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match event {
        MacosEvent::Volume(volume) => Ok(playback
            .transport
            .apply(TransportMessage::SetVolume(volume))),
        MacosEvent::OutputRouteChanged => route_changed(playback, now),
        MacosEvent::HardwareWatchError(detail) => Ok(playback.workspace.show(
            Toast::error("Audio device watch failed").with_text(detail),
            playback.revisions,
        )),
        MacosEvent::MediaKey(request) => playback::update(playback, request, now),
    }
}

fn route_changed(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    if !playback.player.is_playing() {
        return Ok(Cmd::None);
    }
    let paused = playback::update(playback, PlaybackRequest::Pause, now)?;
    let raised = playback.workspace.show(
        Toast::info("Output changed — paused".to_string()),
        playback.revisions,
    );
    Ok(raised.then(paused))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use crate::{
        cmd::{Cmd, Cue, Effect},
        domain::{
            AudioFormat,
            Bounded,
            Model,
            Moment,
            Percent,
            Player,
            Playhead,
            Preload,
            Speed,
            Tags,
            Track,
        },
        message::{MacosEvent, PlaybackRequest},
        update::{macos::update, playback, playback_parts},
    };

    fn playing_model() -> Model {
        let track = Arc::new(
            Track::builder()
                .path("/t.flac")
                .duration(Duration::from_secs(200))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        );
        Model {
            player: Player::Playing {
                track,
                head: Playhead::anchored(
                    Duration::ZERO,
                    Moment::default(),
                    Speed::default(),
                ),
                preload: Preload::None,
            },
            ..Model::default()
        }
    }

    #[test]
    fn volume_sets_the_transport() {
        let mut model = Model::default();
        let volume = Percent::clamped(30);
        let cmd = update(
            &mut playback_parts(&mut model),
            MacosEvent::Volume(volume),
            Moment::default(),
        )
        .unwrap();
        assert_eq!(model.transport.volume, volume);
        assert!(matches!(cmd, Cmd::One(Effect::Animate(Cue::VolumeChanged))));
    }

    #[test]
    fn route_change_while_playing_pauses_and_toasts() {
        let mut model = playing_model();
        let cmd = update(
            &mut playback_parts(&mut model),
            MacosEvent::OutputRouteChanged,
            Moment::default(),
        )
        .unwrap();
        assert!(matches!(model.player, Player::Paused { .. }));
        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.title, "Output changed — paused");
        assert!(cmd.effects().count() > 0);
    }

    #[test]
    fn route_change_while_idle_does_nothing() {
        let mut model = Model::default();
        let cmd = update(
            &mut playback_parts(&mut model),
            MacosEvent::OutputRouteChanged,
            Moment::default(),
        )
        .unwrap();
        assert!(matches!(cmd, Cmd::None));
    }

    #[test]
    fn media_key_toggle_pauses_a_playing_track() {
        let mut via_system = playing_model();
        let mut via_playback = playing_model();
        let now = Moment::default();
        let system_cmd = update(
            &mut playback_parts(&mut via_system),
            MacosEvent::MediaKey(PlaybackRequest::Toggle),
            now,
        )
        .unwrap();
        let playback_cmd = playback::update(
            &mut playback_parts(&mut via_playback),
            PlaybackRequest::Toggle,
            now,
        )
        .unwrap();
        assert_eq!(system_cmd, playback_cmd);
    }
}
