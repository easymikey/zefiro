use crate::{
    cmd::Cmd,
    domain::{Driver, DriverFailure, DriverStatus, Model, Toast},
    message::{DriverMessage, WorkspaceRequest},
    update::{
        machine::{Machine, Rejected},
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverRejection {
    Running,
    Dead,
    Stopped,
}

impl Machine for DriverStatus {
    type Message = DriverMessage;
    type Rejection = DriverRejection;
    type Effect = Option<DriverFailure>;

    fn transition(
        self,
        message: DriverMessage,
    ) -> Result<(Self, Option<DriverFailure>), Rejected<Self>> {
        match (self, message) {
            (DriverStatus::Running, DriverMessage::Died(failure)) => {
                Ok((DriverStatus::Dead(failure.clone()), Some(failure)))
            }
            (DriverStatus::Running | DriverStatus::Dead(_), DriverMessage::Stopped) => {
                Ok((DriverStatus::Stopped, None))
            }
            (state @ DriverStatus::Dead(_), DriverMessage::Died(_)) => Err(Rejected {
                state,
                reason: DriverRejection::Dead,
            }),
            (
                DriverStatus::Stopped,
                DriverMessage::Died(_) | DriverMessage::Stopped,
            ) => Err(Rejected {
                state: DriverStatus::Stopped,
                reason: DriverRejection::Stopped,
            }),
        }
    }
}

pub(super) fn update(
    model: &mut Model,
    driver: Driver,
    message: DriverMessage,
) -> Result<Cmd, Rejection> {
    let died = model
        .drivers
        .status_mut(driver)
        .update(message)
        .map_err(|reason| Rejection::Driver(driver, reason))?;
    let Some(failure) = died else {
        return Ok(Cmd::None);
    };
    Ok(model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::error(format!(
            "The {driver} driver stopped: {failure}"
        ))))?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::{DriverFailure, DriverStatus},
        message::DriverMessage,
        update::{driver::DriverRejection, machine::Machine},
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverFailure::Panicked("boom".to_string()))
    }

    fn died() -> DriverMessage {
        DriverMessage::Died(DriverFailure::Panicked("boom".to_string()))
    }

    struct LifeRow {
        start: DriverStatus,
        message: DriverMessage,
        next: DriverStatus,
        outcome: Result<Option<DriverFailure>, DriverRejection>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        start: DriverStatus::Running,
        message: died(),
        next: dead(),
        outcome: Ok(Some(DriverFailure::Panicked("boom".to_string()))),
    })]
    #[case::running_stops(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        start: dead(),
        message: died(),
        next: dead(),
        outcome: Err(DriverRejection::Dead),
    })]
    #[case::dead_stops(LifeRow {
        start: dead(),
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::stopped_refuses_a_death(LifeRow {
        start: DriverStatus::Stopped,
        message: died(),
        next: DriverStatus::Stopped,
        outcome: Err(DriverRejection::Stopped),
    })]
    #[case::stopped_refuses_a_second_stop(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Err(DriverRejection::Stopped),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.start;
        let outcome = status.update(row.message);
        assert_eq!(status, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
