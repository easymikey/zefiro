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

pub(crate) struct DriverDeath {
    pub(crate) driver_name: DriverName,
    pub(crate) error: DriverError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatusMessage {
    Died(DriverError),
    Stopped,
    Full(DriverName),
}

impl Machine for DriverStatus {
    type Message = DriverStatusMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        driver_status_message: DriverStatusMessage,
    ) -> Result<Cmd, Unhandled> {
        match (&*self, driver_status_message) {
            (DriverStatus::Running, DriverStatusMessage::Died(error)) => {
                *self = DriverStatus::Dead(error);
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
            (
                DriverStatus::Dead(_),
                DriverStatusMessage::Died(_) | DriverStatusMessage::Full(..),
            )
            | (
                DriverStatus::Stopped,
                DriverStatusMessage::Died(_)
                | DriverStatusMessage::Stopped
                | DriverStatusMessage::Full(..),
            ) => Err(Unhandled),
        }
    }
}

impl DriverStatus {
    fn restart(&mut self) {
        *self = DriverStatus::Running;
    }
}

pub(crate) fn update(
    drivers: &mut Drivers,
    driver_name: DriverName,
    event: DriverEvent,
) -> Result<Cmd, Unhandled> {
    let driver_status_message = match event {
        DriverEvent::Died(error) => DriverStatusMessage::Died(error),
        DriverEvent::Stopped => DriverStatusMessage::Stopped,
        DriverEvent::Full => DriverStatusMessage::Full(driver_name),
    };
    drivers
        .record_mut(driver_name)
        .status
        .transition(driver_status_message)
}

pub(crate) fn died(
    drivers: &mut Drivers,
    driver_death: DriverDeath,
    now: Moment,
) -> Result<(Decision, Cmd), Unhandled> {
    let DriverDeath { driver_name, error } = driver_death;
    let died = update(drivers, driver_name, DriverEvent::Died(error))?;
    Ok((restart_if_allowed(drivers, driver_name, now), died))
}

fn restart_if_allowed(
    drivers: &mut Drivers,
    driver_name: DriverName,
    now: Moment,
) -> Decision {
    let decision = decide_restart(
        Supervision::standard(driver_name),
        &drivers.record(driver_name).restarts,
        now,
    );
    match decision {
        Decision::Restart => {
            let restarting = drivers.record_mut(driver_name);
            restarting.status.restart();
            restarting.restarts.record(now);
        }
        Decision::Degrade(_) => {}
    }
    decision
}

pub(crate) struct ResumeParts<'a> {
    pub(crate) player: &'a Player,
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) fn resume_driver(
    parts: ResumeParts<'_>,
    driver_name: DriverName,
    now: Moment,
) -> Cmd {
    let ResumeParts { player, revisions } = parts;
    let (track, playback) = match (driver_name, player) {
        (DriverName::Audio, Player::Playing { track, .. } | Player::Loading(track)) => {
            (track, Playback::Playing)
        }
        (DriverName::Audio, Player::Paused { track, .. }) => (track, Playback::Paused),
        (DriverName::Audio, Player::Stopped)
        | (DriverName::Library | DriverName::Config | DriverName::Macos, _) => {
            return Cmd::none();
        }
    };
    let track_load = TrackLoad::for_track(track, revisions.issue_effect());
    load_from(track_load, player.position_at(now), playback)
}

fn load_from(track_load: TrackLoad, position: Duration, playback: Playback) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Load(track_load)),
        Effect::Audio(AudioCmd::Seek(position)),
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
            driver::{
                DriverDeath,
                DriverStatusMessage,
                ResumeParts,
                died,
                resume_driver,
            },
            machine::{Machine, Unhandled},
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::Panicked)
    }

    fn panicked() -> DriverStatusMessage {
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
        driver_status: DriverStatus,
        driver_status_message: DriverStatusMessage,
        next: DriverStatus,
        result: Result<Cmd, Unhandled>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: panicked(),
        next: dead(),
        result: Ok(Cmd::none()),
    })]
    #[case::running_stops(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::running_is_full(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: filled(),
        next: DriverStatus::Running,
        result: full(),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        driver_status: dead(),
        driver_status_message: panicked(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::dead_stops(LifeRow {
        driver_status: dead(),
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::dead_refuses_full(LifeRow {
        driver_status: dead(),
        driver_status_message: filled(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_death(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: panicked(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_second_stop(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_full(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: filled(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.driver_status;
        let result = status.transition(row.driver_status_message);
        assert_eq!(status, row.next);
        assert_eq!(result, row.result);
    }

    #[test]
    fn dead_restarts() {
        let mut status = dead();

        status.restart();

        assert_eq!(status, DriverStatus::Running);
    }

    #[rstest]
    #[case::a_dead_driver(dead())]
    #[case::a_stopped_driver(DriverStatus::Stopped)]
    fn a_death_the_status_refuses_is_refused_and_not_recorded(
        #[case] status: DriverStatus,
    ) {
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = status;
        let before = drivers.clone();

        let death = died(
            &mut drivers,
            DriverDeath {
                driver_name: DriverName::Audio,
                error: DriverError::Panicked,
            },
            Moment::default(),
        );

        assert_eq!(death, Err(Unhandled));
        assert_eq!(drivers, before);
    }

    #[test]
    fn a_dead_driver_restarts_and_the_restart_is_recorded() {
        let mut drivers = Drivers::default();
        let before = drivers.clone();

        let death = died(
            &mut drivers,
            DriverDeath {
                driver_name: DriverName::Audio,
                error: DriverError::Panicked,
            },
            Moment::default(),
        );

        assert_eq!(death, Ok((Decision::Restart, Cmd::none())));
        assert_eq!(drivers.status(DriverName::Audio), &DriverStatus::Running);
        assert_ne!(
            drivers.record(DriverName::Audio).restarts,
            before.record(DriverName::Audio).restarts
        );
    }

    struct ResumeRow {
        player: Player,
        driver_name: DriverName,
        cmd: Cmd,
    }

    fn track() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/music/a.flac")))
    }

    fn reloaded(position: Duration, playback: Playback) -> Cmd {
        Cmd::from_iter([
            Effect::Audio(AudioCmd::Load(TrackLoad::for_track(
                &track(),
                Revisions::default().issue_effect(),
            ))),
            Effect::Audio(AudioCmd::Seek(position)),
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
        driver_name: DriverName::Audio,
        cmd: reloaded(Duration::from_secs(5), Playback::Playing),
    })]
    #[case::a_loading_track_resumes_playing(ResumeRow {
        player: Player::Loading(track()),
        driver_name: DriverName::Audio,
        cmd: reloaded(Duration::ZERO, Playback::Playing),
    })]
    #[case::a_paused_track_resumes_paused(ResumeRow {
        player: Player::Paused {
            track: track(),
            position: Duration::from_secs(3),
            by: PausedBy::Listener,
        },
        driver_name: DriverName::Audio,
        cmd: reloaded(Duration::from_secs(3), Playback::Paused),
    })]
    #[case::a_stopped_player_resumes_nothing(ResumeRow {
        player: Player::Stopped,
        driver_name: DriverName::Audio,
        cmd: Cmd::none(),
    })]
    #[case::a_library_restart_resumes_nothing(ResumeRow {
        player: Player::Loading(track()),
        driver_name: DriverName::Library,
        cmd: Cmd::none(),
    })]
    fn an_audio_restart_resumes_the_player(#[case] row: ResumeRow) {
        let mut revisions = Revisions::default();
        let effects = resume_driver(
            ResumeParts {
                player: &row.player,
                revisions: &mut revisions,
            },
            row.driver_name,
            Moment::default(),
        );
        assert_eq!(effects, row.cmd);
    }
}
