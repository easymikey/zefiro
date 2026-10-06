use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
    domain::{
        driver::{DriverError, DriverName, DriverStatus, Drivers},
        player::Player,
        revision::Revisions,
        supervision::{Decision, Supervision, decide_restart},
        time::Moment,
        toast::Toast,
    },
    message::{DriverEvent, Message},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatusMessage {
    Died(DriverError),
    Restarted,
    Stopped,
    Full(DriverName),
}

impl Machine for DriverStatus {
    type Message = DriverStatusMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: DriverStatusMessage) -> Result<Cmd, Unhandled> {
        match (&*self, message) {
            (DriverStatus::Running, DriverStatusMessage::Died(failure)) => {
                *self = DriverStatus::Dead(failure);
                Ok(Cmd::none())
            }
            (DriverStatus::Dead(_), DriverStatusMessage::Restarted) => {
                *self = DriverStatus::Running;
                Ok(Cmd::none())
            }
            (
                DriverStatus::Running | DriverStatus::Dead(_),
                DriverStatusMessage::Stopped,
            ) => {
                *self = DriverStatus::Stopped;
                Ok(Cmd::none())
            }
            (DriverStatus::Running, DriverStatusMessage::Full(driver)) => {
                Ok(Cmd::message(Message::Toast(Toast::info(format!(
                    "The {driver} driver is falling behind"
                )))))
            }
            (DriverStatus::Running, DriverStatusMessage::Restarted)
            | (
                DriverStatus::Dead(_),
                DriverStatusMessage::Died(_) | DriverStatusMessage::Full(..),
            )
            | (
                DriverStatus::Stopped,
                DriverStatusMessage::Died(_)
                | DriverStatusMessage::Restarted
                | DriverStatusMessage::Stopped
                | DriverStatusMessage::Full(..),
            ) => Err(Unhandled),
        }
    }
}

pub(crate) fn update(
    drivers: &mut Drivers,
    driver: DriverName,
    event: DriverEvent,
) -> Result<Cmd, Unhandled> {
    let message = match event {
        DriverEvent::Died(failure) => DriverStatusMessage::Died(failure),
        DriverEvent::Stopped => DriverStatusMessage::Stopped,
        DriverEvent::Full => DriverStatusMessage::Full(driver),
    };
    drivers.record_mut(driver).status.transition(message)
}

pub(crate) fn decided(
    drivers: &mut Drivers,
    driver: DriverName,
    now: Moment,
) -> Result<(Decision, Cmd), Unhandled> {
    let decision = decide_restart(
        Supervision::standard(driver),
        &drivers.record(driver).restarts,
        now,
    );
    match decision {
        Decision::Restart => {
            let restarting = drivers.record_mut(driver);
            let restarted = restarting
                .status
                .transition(DriverStatusMessage::Restarted)?;
            restarting.restarts.record(now);
            Ok((Decision::Restart, restarted))
        }
        Decision::Degrade(_) => Ok((decision, Cmd::none())),
    }
}

pub(crate) struct ResumeParts<'a> {
    pub(crate) player: &'a Player,
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) fn resume_driver(
    parts: ResumeParts<'_>,
    driver: DriverName,
    now: Moment,
) -> Cmd {
    let ResumeParts { player, revisions } = parts;
    let (track, playback) = match (driver, player) {
        (DriverName::Audio, Player::Playing { track, .. } | Player::Loading(track)) => {
            (track, Playback::Playing)
        }
        (DriverName::Audio, Player::Paused { track, .. }) => (track, Playback::Paused),
        (DriverName::Audio, Player::Stopped)
        | (DriverName::Library | DriverName::Config | DriverName::Macos, _) => {
            return Cmd::none();
        }
    };
    let request = TrackLoad::for_track(track, revisions.issue_effect());
    load_at(request, player.position_at(now), playback)
}

fn load_at(request: TrackLoad, at: Duration, playback: Playback) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Load(request)),
        Effect::Audio(AudioCmd::Seek(at)),
        Effect::Audio(AudioCmd::SetPlayback(playback)),
    ])
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
        domain::{
            driver::{DriverError, DriverName, DriverStatus, Drivers},
            player::{PausedBy, Player},
            playhead::Playhead,
            revision::Revisions,
            speed::Speed,
            supervision::Decision,
            time::Moment,
            toast::Toast,
            track::Track,
        },
        message::Message,
        update::{
            driver::{DriverStatusMessage, ResumeParts, decided, resume_driver},
            machine::{Machine, Unhandled},
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::Panicked)
    }

    fn died() -> DriverStatusMessage {
        DriverStatusMessage::Died(DriverError::Panicked)
    }

    fn filled() -> DriverStatusMessage {
        DriverStatusMessage::Full(DriverName::Audio)
    }

    fn full() -> Result<Cmd, Unhandled> {
        Ok(Cmd::message(Message::Toast(Toast::info(
            "The audio driver is falling behind".to_string(),
        ))))
    }

    struct LifeRow {
        start: DriverStatus,
        message: DriverStatusMessage,
        next: DriverStatus,
        result: Result<Cmd, Unhandled>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        start: DriverStatus::Running,
        message: died(),
        next: dead(),
        result: Ok(Cmd::none()),
    })]
    #[case::running_stops(LifeRow {
        start: DriverStatus::Running,
        message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::running_is_full(LifeRow {
        start: DriverStatus::Running,
        message: filled(),
        next: DriverStatus::Running,
        result: full(),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        start: dead(),
        message: died(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::dead_stops(LifeRow {
        start: dead(),
        message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::dead_refuses_full(LifeRow {
        start: dead(),
        message: filled(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::dead_restarts(LifeRow {
        start: dead(),
        message: DriverStatusMessage::Restarted,
        next: DriverStatus::Running,
        result: Ok(Cmd::none()),
    })]
    #[case::running_refuses_a_restart(LifeRow {
        start: DriverStatus::Running,
        message: DriverStatusMessage::Restarted,
        next: DriverStatus::Running,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_restart(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverStatusMessage::Restarted,
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_death(LifeRow {
        start: DriverStatus::Stopped,
        message: died(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_second_stop(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_full(LifeRow {
        start: DriverStatus::Stopped,
        message: filled(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.start;
        let result = status.transition(row.message);
        assert_eq!(status, row.next);
        assert_eq!(result, row.result);
    }

    #[rstest]
    #[case::a_running_driver(DriverStatus::Running)]
    #[case::a_stopped_driver(DriverStatus::Stopped)]
    fn a_restart_the_status_refuses_is_refused_and_not_recorded(
        #[case] status: DriverStatus,
    ) {
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = status;
        let before = drivers.clone();

        let decision = decided(&mut drivers, DriverName::Audio, Moment::default());

        assert_eq!(decision, Err(Unhandled));
        assert_eq!(drivers, before);
    }

    #[test]
    fn a_dead_driver_restarts_and_the_restart_is_recorded() {
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = dead();
        let before = drivers.clone();

        let decision = decided(&mut drivers, DriverName::Audio, Moment::default());

        assert_eq!(decision, Ok((Decision::Restart, Cmd::none())));
        assert_eq!(drivers.status(DriverName::Audio), &DriverStatus::Running);
        assert_ne!(
            drivers.record(DriverName::Audio).restarts,
            before.record(DriverName::Audio).restarts
        );
    }

    struct ResumeRow {
        player: Player,
        driver: DriverName,
        effects: Cmd,
    }

    fn track() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/music/a.flac")))
    }

    fn reloaded(at: Duration, playback: Playback) -> Cmd {
        Cmd::from_iter([
            Effect::Audio(AudioCmd::Load(TrackLoad::for_track(
                &track(),
                Revisions::default().issue_effect(),
            ))),
            Effect::Audio(AudioCmd::Seek(at)),
            Effect::Audio(AudioCmd::SetPlayback(playback)),
        ])
    }

    #[rstest]
    #[case::a_playing_track_resumes_playing(ResumeRow {
        player: Player::Playing {
            track: track(),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        driver: DriverName::Audio,
        effects: reloaded(Duration::from_secs(5), Playback::Playing),
    })]
    #[case::a_loading_track_resumes_playing(ResumeRow {
        player: Player::Loading(track()),
        driver: DriverName::Audio,
        effects: reloaded(Duration::ZERO, Playback::Playing),
    })]
    #[case::a_paused_track_resumes_paused(ResumeRow {
        player: Player::Paused {
            track: track(),
            position: Duration::from_secs(3),
            by: PausedBy::Listener,
        },
        driver: DriverName::Audio,
        effects: reloaded(Duration::from_secs(3), Playback::Paused),
    })]
    #[case::a_stopped_player_resumes_nothing(ResumeRow {
        player: Player::Stopped,
        driver: DriverName::Audio,
        effects: Cmd::none(),
    })]
    #[case::a_library_restart_resumes_nothing(ResumeRow {
        player: Player::Loading(track()),
        driver: DriverName::Library,
        effects: Cmd::none(),
    })]
    fn an_audio_restart_resumes_the_player(#[case] row: ResumeRow) {
        let mut revisions = Revisions::default();
        let effects = resume_driver(
            ResumeParts {
                player: &row.player,
                revisions: &mut revisions,
            },
            row.driver,
            Moment::default(),
        );
        assert_eq!(effects, row.effects);
    }
}
