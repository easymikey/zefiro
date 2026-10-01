use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback, TrackRequest},
    domain::{
        Announce,
        Decision,
        Driver,
        DriverError,
        DriverStatus,
        Model,
        Moment,
        Player,
        Toast,
        supervise,
    },
    message::DriverMessage,
    update::{
        error::UpdateError,
        machine::{Machine, Rejected},
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

pub(crate) struct Died {
    pub(crate) driver: Driver,
    pub(crate) failure: DriverError,
}

pub(crate) fn update(
    model: &mut Model,
    driver: Driver,
    event: DriverMessage,
) -> Result<Option<DriverSignal>, UpdateError> {
    model
        .drivers
        .record_mut(driver)
        .status
        .update(event)
        .map_err(|reason| UpdateError::Driver(driver, reason))
}

pub(crate) fn inbox_full(model: &mut Model, driver: Driver) -> Cmd {
    model.workspace.show(
        Toast::info(format!("The {driver} driver is falling behind")),
        &mut model.revisions,
    )
}

pub(crate) fn decided(model: &mut Model, died: Died, now: Moment) -> Cmd {
    let Died { driver, failure } = died;
    let record = model.drivers.record(driver);
    let decision = supervise(record.supervision, &record.restarts, now);
    match decision {
        Decision::Restart => {
            let restarting = model.drivers.record_mut(driver);
            restarting.restarts.record(now);
            restarting.status = DriverStatus::Running;
            restarted(model, driver, now)
        }
        Decision::Degrade(Announce::Toast) => model.workspace.show(
            Toast::error(format!("The {driver} driver stopped: {failure}")),
            &mut model.revisions,
        ),
        Decision::Degrade(Announce::Silent) => Cmd::None,
    }
}

fn restarted(model: &mut Model, driver: Driver, now: Moment) -> Cmd {
    Cmd::from(Effect::Restart(driver))
        .then(startup_cmd(model, driver))
        .then(resume(model, driver, now))
}

fn resume(model: &mut Model, driver: Driver, now: Moment) -> Cmd {
    let playback = match &model.player {
        Player::Playing { .. } => Some(Playback::Playing),
        Player::Paused { .. } => Some(Playback::Paused),
        Player::Stopped | Player::Loading { .. } => None,
    };
    let (Driver::Audio, Some(track), Some(playback)) =
        (driver, model.player.current(), playback)
    else {
        return Cmd::None;
    };
    let request = TrackRequest::for_track(track, model.revisions.issue_effect());
    load_at(request, model.player.position_at(now), playback)
}

fn load_at(request: TrackRequest, at: Duration, playback: Playback) -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Load(request)),
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
