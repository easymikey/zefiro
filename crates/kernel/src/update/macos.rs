use crate::{
    cmd::Cmd,
    domain::{time::Moment, toast::Toast},
    message::{MacosEvent, PlaybackRequest},
    update::{
        machine::{Machine, Unhandled},
        playback,
        player::PlaybackParts,
        transport::TransportMessage,
    },
};

pub(crate) fn update(
    playback: &mut PlaybackParts<'_>,
    event: MacosEvent,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match event {
        MacosEvent::Volume(volume) => playback
            .transport
            .transition(TransportMessage::SetVolume(volume)),
        MacosEvent::OutputRouteChanged => route_changed(playback, now),
        MacosEvent::Error(error) => Ok(playback
            .workspace
            .show(Toast::error(error.to_string()), playback.revisions)),
        MacosEvent::MediaKey(request) => playback::update(playback, request, now),
    }
}

fn route_changed(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if !playback.player.is_playing() {
        return Ok(Cmd::none());
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
            bounded::Bounded,
            model::Model,
            percent::Percent,
            player::{Player, Preload},
            playhead::Playhead,
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track},
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
        assert_eq!(cmd, Cmd::effect(Effect::Animate(Cue::VolumeChanged)));
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
        assert!(cmd == Cmd::none());
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
