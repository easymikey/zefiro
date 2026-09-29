use crate::{
    cmd::Cmd,
    domain::{Model, Moment, Toast},
    message::{SystemEvent, WorkspaceRequest},
    update::{
        machine::Machine,
        playback,
        player::{self, Anchor, PlayerMessage, Resume},
        rejection::Rejection,
        transport::TransportMessage,
    },
};

pub(crate) fn system(
    model: &mut Model,
    event: SystemEvent,
    now: Moment,
) -> Result<Cmd, Rejection> {
    match event {
        SystemEvent::Volume(volume) => Ok(model
            .transport
            .update(TransportMessage::SetVolume(volume))?),
        SystemEvent::OutputRouteChanged => route_changed(model, now),
        SystemEvent::HardwareWatchFailed(detail) => {
            Ok(model
                .workspace
                .update(WorkspaceRequest::ShowToast(Toast::error(format!(
                    "Audio device watch failed: {detail}"
                ))))?)
        }
        SystemEvent::MediaKey(gesture) => {
            playback::playback(model, gesture.into(), now)
        }
    }
}

fn route_changed(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    if !model.player.is_playing() {
        return Ok(Cmd::None);
    }
    let current = model.playlist.current().cloned();
    let resume = Resume {
        anchor: Anchor {
            now,
            speed: model.transport.speed,
        },
    };
    let paused =
        player::account(model, PlayerMessage::Toggle { current, resume }, now)?;
    let raised = model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::info(
            "Output changed — paused".to_string(),
        )))?;
    Ok(raised.then(paused))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

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
        message::{Gesture, PlaybackRequest, SystemEvent},
        update::{playback::playback, system::system},
    };

    #[rstest]
    #[case::play(Gesture::Play, PlaybackRequest::Play)]
    #[case::pause(Gesture::Pause, PlaybackRequest::Pause)]
    #[case::toggle(Gesture::Toggle, PlaybackRequest::Toggle)]
    #[case::stop(Gesture::Stop, PlaybackRequest::Stop)]
    #[case::next(Gesture::Next, PlaybackRequest::Next)]
    #[case::previous(Gesture::Previous, PlaybackRequest::Prev)]
    #[case::seek_forward(Gesture::SeekForward, PlaybackRequest::SeekForward)]
    #[case::seek_back(Gesture::SeekBack, PlaybackRequest::SeekBack)]
    #[case::scrub(
        Gesture::Scrub(Duration::from_millis(42_500)),
        PlaybackRequest::SeekTo(Duration::from_millis(42_500))
    )]
    fn a_gesture_maps_to_its_request(
        #[case] gesture: Gesture,
        #[case] expected: PlaybackRequest,
    ) {
        assert_eq!(PlaybackRequest::from(gesture), expected);
    }

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
        let cmd =
            system(&mut model, SystemEvent::Volume(volume), Moment::default()).unwrap();
        assert_eq!(model.transport.volume, volume);
        assert!(matches!(cmd, Cmd::One(Effect::Animate(Cue::VolumeChanged))));
    }

    #[test]
    fn route_change_while_playing_pauses_and_toasts() {
        let mut model = playing_model();
        let cmd = system(
            &mut model,
            SystemEvent::OutputRouteChanged,
            Moment::default(),
        )
        .unwrap();
        assert!(matches!(model.player, Player::Paused { .. }));
        let toast = model.workspace.toast.unwrap();
        assert_eq!(toast.text, "Output changed — paused");
        assert!(cmd.effects().count() > 0);
    }

    #[test]
    fn route_change_while_idle_does_nothing() {
        let mut model = Model::default();
        let cmd = system(
            &mut model,
            SystemEvent::OutputRouteChanged,
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
        let system_cmd =
            system(&mut via_system, SystemEvent::MediaKey(Gesture::Toggle), now)
                .unwrap();
        let playback_cmd =
            playback(&mut via_playback, PlaybackRequest::Toggle, now).unwrap();
        assert_eq!(system_cmd, playback_cmd);
    }
}
