use crate::{
    cmd::Cmd,
    domain::{time::Moment, toast::Toast},
    message::{MacosEvent, PlaybackRequest},
    update::{
        machine::{Machine, Unhandled},
        playback,
        player::events::PlaybackParts,
        transport::TransportMessage,
    },
};

pub(crate) fn update(
    playback_parts: &mut PlaybackParts<'_>,
    event: MacosEvent,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match event {
        MacosEvent::VolumeChanged(volume) => playback_parts
            .transport
            .transition(TransportMessage::SetVolume(volume)),
        MacosEvent::OutputRouteChanged => route_changed(playback_parts, now),
        MacosEvent::Error(error) => Ok(playback_parts
            .workspace
            .show(Toast::error(error.to_string()), playback_parts.revisions)),
        MacosEvent::MediaKeyPressed(request) => {
            playback::update(playback_parts, request, now)
        }
    }
}

fn route_changed(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let paused = playback::update(playback_parts, PlaybackRequest::Pause, now)?;
    let raised = playback_parts.workspace.show(
        Toast::info("Output changed — paused".to_string()),
        playback_parts.revisions,
    );
    Ok(raised.then(paused))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use crate::{
        cmd::{Cmd, Effect},
        domain::{
            bounded::Bounded,
            cue::Cue,
            model::Model,
            percent::Percent,
            player::Player,
            playhead::Playhead,
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track, TrackParts},
        },
        message::{MacosEvent, PlaybackRequest},
        update::{machine::Unhandled, macos::update, playback, playback_parts},
    };

    fn playing_model() -> Model {
        let track = Arc::new(Track::new(TrackParts {
            path: "/t.flac".into(),
            duration: Duration::from_secs(200),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        }));
        Model {
            player: Player::Playing {
                track,
                playhead: Playhead::anchored(
                    Duration::ZERO,
                    Moment::default(),
                    Speed::default(),
                ),
                preloaded: None,
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
            MacosEvent::VolumeChanged(volume),
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
    fn route_change_while_idle_is_refused() {
        let mut model = Model::default();
        let result = update(
            &mut playback_parts(&mut model),
            MacosEvent::OutputRouteChanged,
            Moment::default(),
        );
        assert_eq!(result, Err(Unhandled));
    }

    #[test]
    fn media_key_toggle_pauses_a_playing_track() {
        let mut via_system = playing_model();
        let mut via_playback = playing_model();
        let now = Moment::default();
        let system_cmd = update(
            &mut playback_parts(&mut via_system),
            MacosEvent::MediaKeyPressed(PlaybackRequest::Toggle),
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
