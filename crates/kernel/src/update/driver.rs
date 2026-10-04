use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
    domain::{
        Announce,
        Decision,
        DriverError,
        DriverName,
        DriverStatus,
        Drivers,
        Moment,
        Player,
        Revisions,
        Supervision,
        Toast,
        Workspace,
        decide_restart,
    },
    message::{DriverEvent, Message},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatusMessage {
    Died {
        driver: DriverName,
        failure: DriverError,
    },
    Stopped,
    Full(DriverName),
}

impl Machine for DriverStatus {
    type Message = DriverStatusMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: DriverStatusMessage) -> Result<Cmd, Unhandled> {
        match (&*self, message) {
            (DriverStatus::Running, DriverStatusMessage::Died { driver, failure }) => {
                *self = DriverStatus::Dead(failure);
                Ok(Cmd::message(Message::DriverDied(driver)))
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
                DriverStatusMessage::Died { .. } | DriverStatusMessage::Full(..),
            )
            | (
                DriverStatus::Stopped,
                DriverStatusMessage::Died { .. }
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
        DriverEvent::Died(failure) => DriverStatusMessage::Died { driver, failure },
        DriverEvent::Stopped => DriverStatusMessage::Stopped,
        DriverEvent::Full => DriverStatusMessage::Full(driver),
    };
    drivers.record_mut(driver).status.transition(message)
}

pub(crate) struct DriverParts<'a> {
    pub(crate) drivers: &'a mut Drivers,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) enum Restart {
    Granted,
    Declined(Cmd),
}

pub(crate) fn decided(
    parts: DriverParts<'_>,
    driver: DriverName,
    now: Moment,
) -> Restart {
    let DriverParts {
        drivers,
        workspace,
        revisions,
    } = parts;
    let record = drivers.record(driver);
    let decision = decide_restart(Supervision::standard(driver), &record.restarts, now);
    match decision {
        Decision::Restart => {
            let restarting = drivers.record_mut(driver);
            restarting.restarts.record(now);
            restarting.status = DriverStatus::Running;
            Restart::Granted
        }
        Decision::Degrade(Announce::Toast) => match &record.status {
            DriverStatus::Dead(failure) => Restart::Declined(
                workspace.show(
                    Toast::error(format!("The {driver} driver stopped"))
                        .with_text(failure.to_string()),
                    revisions,
                ),
            ),
            DriverStatus::Running | DriverStatus::Stopped => {
                Restart::Declined(Cmd::none())
            }
        },
        Decision::Degrade(Announce::Silent) => Restart::Declined(Cmd::none()),
    }
}

pub(crate) fn resumed(
    player: &Player,
    revisions: &mut Revisions,
    restart: (DriverName, Moment),
) -> Cmd {
    let (driver, now) = restart;
    match driver {
        DriverName::Audio => resume(player, revisions, now),
        DriverName::Library | DriverName::Config | DriverName::Macos => Cmd::none(),
    }
}

fn resume(player: &Player, revisions: &mut Revisions, now: Moment) -> Cmd {
    let playback = match player {
        Player::Playing { .. } => Playback::Playing,
        Player::Paused { .. } => Playback::Paused,
        Player::Stopped | Player::Loading { .. } => return Cmd::none(),
    };
    let Some(track) = player.current() else {
        return Cmd::none();
    };
    let request = TrackLoad::for_track(track, revisions.issue_effect());
    load_at(request, player.position_at(now), playback)
}

fn load_at(request: TrackLoad, at: Duration, playback: Playback) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Load(request)),
        Effect::Audio(AudioCmd::Seek(at)),
        Effect::Audio(AudioCmd::Playback(playback)),
    ])
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        cmd::Cmd,
        domain::{DriverError, DriverName, DriverStatus, Toast},
        message::Message,
        update::{
            driver::DriverStatusMessage,
            machine::{Machine, Unhandled},
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::Panicked)
    }

    fn died() -> DriverStatusMessage {
        DriverStatusMessage::Died {
            driver: DriverName::Audio,
            failure: DriverError::Panicked,
        }
    }

    fn filled() -> DriverStatusMessage {
        DriverStatusMessage::Full(DriverName::Audio)
    }

    fn decide_restart() -> Result<Cmd, Unhandled> {
        Ok(Cmd::message(Message::DriverDied(DriverName::Audio)))
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
        result: decide_restart(),
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
    #[case::dead_refuses_congestion(LifeRow {
        start: dead(),
        message: filled(),
        next: dead(),
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
    #[case::stopped_refuses_congestion(LifeRow {
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
}
