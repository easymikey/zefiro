use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback},
    domain::{
        Announce,
        Decision,
        Driver,
        DriverError,
        DriverStatus,
        Model,
        Moment,
        Player,
        Revision,
        Toast,
        Track,
        Workspace,
        supervise,
    },
    message::{DriverMessage, Timer, WorkspaceRequest},
    update::{
        error::UpdateError,
        machine::{Machine, Rejected},
        quit,
        startup::startup_cmd,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatusError {
    Running,
    Dead,
    Stopped,
    Input(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverSignal {
    Died(DriverError),
    Congested,
}

impl Machine for DriverStatus {
    type Message = DriverMessage;
    type Error = DriverStatusError;
    type Effect = Option<DriverSignal>;

    fn transition(
        self,
        message: DriverMessage,
    ) -> Result<(Self, Option<DriverSignal>), Rejected<Self>> {
        match (self, message) {
            (state, DriverMessage::Rejected { input }) => Err(Rejected {
                state,
                reason: DriverStatusError::Input(input),
            }),
            (DriverStatus::Running, DriverMessage::Died(failure)) => Ok((
                DriverStatus::Dead(failure.clone()),
                Some(DriverSignal::Died(failure)),
            )),
            (DriverStatus::Running | DriverStatus::Dead(_), DriverMessage::Stopped) => {
                Ok((DriverStatus::Stopped, None))
            }
            (DriverStatus::Running, DriverMessage::Full) => {
                Ok((DriverStatus::Running, Some(DriverSignal::Congested)))
            }
            (
                state @ DriverStatus::Dead(_),
                DriverMessage::Died(_) | DriverMessage::Full,
            ) => Err(Rejected {
                state,
                reason: DriverStatusError::Dead,
            }),
            (
                DriverStatus::Stopped,
                DriverMessage::Died(_) | DriverMessage::Stopped | DriverMessage::Full,
            ) => Err(Rejected {
                state: DriverStatus::Stopped,
                reason: DriverStatusError::Stopped,
            }),
        }
    }
}

struct Died {
    driver: Driver,
    failure: DriverError,
}

pub(crate) fn update(
    model: &mut Model,
    message: (Driver, DriverMessage),
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let (driver, driver_message) = message;
    let signal = model
        .drivers
        .record_mut(driver)
        .status
        .update(driver_message)
        .map_err(|reason| UpdateError::Driver(driver, reason))?;
    match signal {
        Some(DriverSignal::Died(failure)) => {
            Ok(decided(model, Died { driver, failure }, now))
        }
        Some(DriverSignal::Congested) => Ok(show_toast(
            &mut model.workspace,
            Toast::info(format!("The {driver} driver is falling behind")),
        )),
        None => Ok(Cmd::None),
    }
}

pub(crate) fn restart_due(model: &mut Model, driver: Driver, now: Moment) -> Cmd {
    match model.drivers.status(driver) {
        DriverStatus::Dead(_) => {
            model.drivers.record_mut(driver).status = DriverStatus::Running;
            restarted(model, driver, now)
        }
        DriverStatus::Running | DriverStatus::Stopped => Cmd::None,
    }
}

fn decided(model: &mut Model, died: Died, now: Moment) -> Cmd {
    let Died { driver, failure } = died;
    let record = model.drivers.record(driver);
    let decision = supervise(record.strategy, &record.restarts, now);
    match decision {
        Decision::Restart => {
            let restarting = model.drivers.record_mut(driver);
            restarting.restarts.record(now);
            restarting.status = DriverStatus::Running;
            restarted(model, driver, now)
        }
        Decision::RestartAfter(delay) => {
            model.drivers.record_mut(driver).restarts.record(now);
            Effect::After {
                delay,
                message: Timer::Restart(driver),
            }
            .into()
        }
        Decision::Degrade(Announce::Toast) => show_toast(
            &mut model.workspace,
            Toast::error(format!("The {driver} driver stopped: {failure}")),
        ),
        Decision::Degrade(Announce::Silent) => Cmd::None,
        Decision::Quit => quit(),
    }
}

fn restarted(model: &Model, driver: Driver, now: Moment) -> Cmd {
    Cmd::from(Effect::Restart(driver))
        .then(startup_cmd(model, driver))
        .then(resume(model, driver, now))
}

fn show_toast(workspace: &mut Workspace, toast: Toast) -> Cmd {
    match workspace.update(WorkspaceRequest::ShowToast(toast)) {
        Ok(cmd) => cmd,
        Err(never) => match never {},
    }
}

fn resume(model: &Model, driver: Driver, now: Moment) -> Cmd {
    if driver != Driver::Audio {
        return Cmd::None;
    }
    let playback = match &model.player {
        Player::Playing { .. } => Playback::Playing,
        Player::Paused { .. } => Playback::Paused,
        Player::Stopped | Player::Loading { .. } => return Cmd::None,
    };
    let Some(track) = model.player.current() else {
        return Cmd::None;
    };
    loaded_at(track, model.player.position_at(now), playback)
}

fn loaded_at(track: &Arc<Track>, at: Duration, playback: Playback) -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Load {
            path: track.path().to_path_buf(),
            gain: track.audio_format().replay_gain,
            revision: Revision::UNSTAMPED,
        }),
        Effect::Audio(AudioCmd::Seek(at)),
        Effect::Audio(AudioCmd::Playback(playback)),
    ])
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::{DriverError, DriverStatus},
        message::DriverMessage,
        update::{
            driver::{DriverSignal, DriverStatusError},
            machine::Machine,
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::Panicked("boom".to_string()))
    }

    fn died() -> DriverMessage {
        DriverMessage::Died(DriverError::Panicked("boom".to_string()))
    }

    struct LifeRow {
        start: DriverStatus,
        message: DriverMessage,
        next: DriverStatus,
        outcome: Result<Option<DriverSignal>, DriverStatusError>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        start: DriverStatus::Running,
        message: died(),
        next: dead(),
        outcome: Ok(Some(DriverSignal::Died(DriverError::Panicked("boom".to_string())))),
    })]
    #[case::running_stops(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::running_is_congested(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Full,
        next: DriverStatus::Running,
        outcome: Ok(Some(DriverSignal::Congested)),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        start: dead(),
        message: died(),
        next: dead(),
        outcome: Err(DriverStatusError::Dead),
    })]
    #[case::dead_stops(LifeRow {
        start: dead(),
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::dead_refuses_congestion(LifeRow {
        start: dead(),
        message: DriverMessage::Full,
        next: dead(),
        outcome: Err(DriverStatusError::Dead),
    })]
    #[case::stopped_refuses_a_death(LifeRow {
        start: DriverStatus::Stopped,
        message: died(),
        next: DriverStatus::Stopped,
        outcome: Err(DriverStatusError::Stopped),
    })]
    #[case::stopped_refuses_a_second_stop(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Err(DriverStatusError::Stopped),
    })]
    #[case::stopped_refuses_congestion(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverMessage::Full,
        next: DriverStatus::Stopped,
        outcome: Err(DriverStatusError::Stopped),
    })]
    #[case::running_reports_a_rejected_input(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Rejected { input: "seek" },
        next: DriverStatus::Running,
        outcome: Err(DriverStatusError::Input("seek")),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.start;
        let outcome = status.update(row.message);
        assert_eq!(status, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
