use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
    domain::{
        Announce,
        Decision,
        Driver,
        DriverStatus,
        Drivers,
        Model,
        Moment,
        Player,
        Revisions,
        Supervision,
        Toast,
        supervise,
    },
    message::DriverEvent,
    update::{error::UpdateError, machine::Machine, startup::startup_cmd},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DriverStatusError {
    #[error("driver already running")]
    Running,
    #[error("driver is dead")]
    Dead,
    #[error("driver is stopped")]
    Stopped,
    #[error("driver rejected input {0}")]
    Input(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverSignal {
    Died,
    Full,
}

impl Machine for DriverStatus {
    type Message = DriverEvent;
    type Error = DriverStatusError;
    type Effect = Option<DriverSignal>;

    fn transition(
        &mut self,
        message: DriverEvent,
    ) -> Result<Option<DriverSignal>, DriverStatusError> {
        match (&*self, message) {
            (_, DriverEvent::Rejected { input }) => {
                Err(DriverStatusError::Input(input))
            }
            (DriverStatus::Running, DriverEvent::Died(failure)) => {
                *self = DriverStatus::Dead(failure);
                Ok(Some(DriverSignal::Died))
            }
            (DriverStatus::Running | DriverStatus::Dead(_), DriverEvent::Stopped) => {
                *self = DriverStatus::Stopped;
                Ok(None)
            }
            (DriverStatus::Running, DriverEvent::Full) => Ok(Some(DriverSignal::Full)),
            (DriverStatus::Dead(_), DriverEvent::Died(_) | DriverEvent::Full) => {
                Err(DriverStatusError::Dead)
            }
            (
                DriverStatus::Stopped,
                DriverEvent::Died(_) | DriverEvent::Stopped | DriverEvent::Full,
            ) => Err(DriverStatusError::Stopped),
        }
    }
}

pub(crate) fn update(
    drivers: &mut Drivers,
    driver: Driver,
    event: DriverEvent,
) -> Result<Option<DriverSignal>, UpdateError> {
    drivers
        .record_mut(driver)
        .status
        .transition(event)
        .map_err(|reason| UpdateError::Driver(driver, reason))
}

pub(crate) fn decided(model: &mut Model, driver: Driver, now: Moment) -> Cmd {
    let Model {
        drivers,
        workspace,
        revisions,
        ..
    } = &mut *model;
    let record = drivers.record(driver);
    let decision = supervise(Supervision::standard(driver), &record.restarts, now);
    match decision {
        Decision::Restart => {
            let restarting = drivers.record_mut(driver);
            restarting.restarts.record(now);
            restarting.status = DriverStatus::Running;
            restarted(model, driver, now)
        }
        Decision::Degrade(Announce::Toast) => match &record.status {
            DriverStatus::Dead(failure) => workspace.show(
                Toast::error(format!("The {driver} driver stopped"))
                    .with_text(failure.to_string()),
                revisions,
            ),
            DriverStatus::Running | DriverStatus::Stopped => Cmd::None,
        },
        Decision::Degrade(Announce::Silent) => Cmd::None,
    }
}

fn restarted(model: &mut Model, driver: Driver, now: Moment) -> Cmd {
    let startup = startup_cmd(model, driver);
    let Model {
        player, revisions, ..
    } = model;
    let resumed = match driver {
        Driver::Audio => resume(player, revisions, now),
        Driver::Library | Driver::Config | Driver::Macos => Cmd::None,
    };
    Cmd::from(Effect::Restart(driver))
        .then(startup)
        .then(resumed)
}

fn resume(player: &Player, revisions: &mut Revisions, now: Moment) -> Cmd {
    let playback = match player {
        Player::Playing { .. } => Playback::Playing,
        Player::Paused { .. } => Playback::Paused,
        Player::Stopped | Player::Loading { .. } => return Cmd::None,
    };
    let Some(track) = player.current() else {
        return Cmd::None;
    };
    let request = TrackLoad::for_track(track, revisions.issue_effect());
    load_at(request, player.position_at(now), playback)
}

fn load_at(request: TrackLoad, at: Duration, playback: Playback) -> Cmd {
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
        message::DriverEvent,
        update::{
            driver::{DriverSignal, DriverStatusError},
            machine::Machine,
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::panicked("boom".to_string()))
    }

    fn died() -> DriverEvent {
        DriverEvent::Died(DriverError::panicked("boom".to_string()))
    }

    struct LifeRow {
        start: DriverStatus,
        message: DriverEvent,
        next: DriverStatus,
        outcome: Result<Option<DriverSignal>, DriverStatusError>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        start: DriverStatus::Running,
        message: died(),
        next: dead(),
        outcome: Ok(Some(DriverSignal::Died)),
    })]
    #[case::running_stops(LifeRow {
        start: DriverStatus::Running,
        message: DriverEvent::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::running_is_congested(LifeRow {
        start: DriverStatus::Running,
        message: DriverEvent::Full,
        next: DriverStatus::Running,
        outcome: Ok(Some(DriverSignal::Full)),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        start: dead(),
        message: died(),
        next: dead(),
        outcome: Err(DriverStatusError::Dead),
    })]
    #[case::dead_stops(LifeRow {
        start: dead(),
        message: DriverEvent::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::dead_refuses_congestion(LifeRow {
        start: dead(),
        message: DriverEvent::Full,
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
        message: DriverEvent::Stopped,
        next: DriverStatus::Stopped,
        outcome: Err(DriverStatusError::Stopped),
    })]
    #[case::stopped_refuses_congestion(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverEvent::Full,
        next: DriverStatus::Stopped,
        outcome: Err(DriverStatusError::Stopped),
    })]
    #[case::running_reports_a_rejected_input(LifeRow {
        start: DriverStatus::Running,
        message: DriverEvent::Rejected { input: "seek" },
        next: DriverStatus::Running,
        outcome: Err(DriverStatusError::Input("seek")),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.start;
        let outcome = status.transition(row.message);
        assert_eq!(status, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
