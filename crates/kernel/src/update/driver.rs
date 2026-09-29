use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{
        AudioCmd,
        Cmd,
        ConfigCmd,
        Effect,
        LibraryCmd,
        NowPlaying,
        Playback,
        SystemCmd,
    },
    domain::{
        Decision,
        Driver,
        DriverFailure,
        DriverStatus,
        Model,
        Moment,
        Notice,
        Player,
        Revision,
        Toast,
        Track,
        Workspace,
        supervise,
    },
    message::{DriverMessage, Timer, WorkspaceRequest},
    update::{
        machine::{Machine, Rejected},
        quit,
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverRejection {
    Running,
    Dead,
    Stopped,
    Input(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverSignal {
    Died(DriverFailure),
    Congested,
}

impl Machine for DriverStatus {
    type Message = DriverMessage;
    type Rejection = DriverRejection;
    type Effect = Option<DriverSignal>;

    fn transition(
        self,
        message: DriverMessage,
    ) -> Result<(Self, Option<DriverSignal>), Rejected<Self>> {
        match (self, message) {
            (state, DriverMessage::Rejected { input }) => Err(Rejected {
                state,
                reason: DriverRejection::Input(input),
            }),
            (DriverStatus::Running, DriverMessage::Died(failure)) => Ok((
                DriverStatus::Dead(failure.clone()),
                Some(DriverSignal::Died(failure)),
            )),
            (DriverStatus::Running | DriverStatus::Dead(_), DriverMessage::Stopped) => {
                Ok((DriverStatus::Stopped, None))
            }
            (DriverStatus::Running, DriverMessage::Congested) => {
                Ok((DriverStatus::Running, Some(DriverSignal::Congested)))
            }
            (
                state @ DriverStatus::Dead(_),
                DriverMessage::Died(_) | DriverMessage::Congested,
            ) => Err(Rejected {
                state,
                reason: DriverRejection::Dead,
            }),
            (
                DriverStatus::Stopped,
                DriverMessage::Died(_)
                | DriverMessage::Stopped
                | DriverMessage::Congested,
            ) => Err(Rejected {
                state: DriverStatus::Stopped,
                reason: DriverRejection::Stopped,
            }),
        }
    }
}

struct Died {
    driver: Driver,
    failure: DriverFailure,
}

pub(crate) fn update(
    model: &mut Model,
    message: (Driver, DriverMessage),
    now: Moment,
) -> Result<Cmd, Rejection> {
    let (driver, driver_message) = message;
    let signal = model
        .drivers
        .record_mut(driver)
        .status
        .update(driver_message)
        .map_err(|reason| Rejection::Driver(driver, reason))?;
    match signal {
        Some(DriverSignal::Died(failure)) => {
            Ok(decided(model, Died { driver, failure }, now))
        }
        Some(DriverSignal::Congested) => Ok(shown(
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
        Decision::Degrade(Notice::Toast) => shown(
            &mut model.workspace,
            Toast::error(format!("The {driver} driver stopped: {failure}")),
        ),
        Decision::Degrade(Notice::Silent) => Cmd::None,
        Decision::Quit => quit(),
    }
}

fn restarted(model: &Model, driver: Driver, now: Moment) -> Cmd {
    Cmd::from(Effect::Restart(driver))
        .then(boot(model, driver))
        .then(resume(model, driver, now))
}

fn shown(workspace: &mut Workspace, toast: Toast) -> Cmd {
    match workspace.update(WorkspaceRequest::ShowToast(toast)) {
        Ok(cmd) => cmd,
        Err(never) => match never {},
    }
}

fn boot(model: &Model, driver: Driver) -> Cmd {
    match driver {
        Driver::Audio => Cmd::Batch(vec![
            Effect::Audio(AudioCmd::ListDevices),
            Effect::Audio(AudioCmd::SetDevice(model.settings.output_device.clone())),
            Effect::Audio(AudioCmd::SetCrossfade(model.settings.crossfade)),
            Effect::Audio(AudioCmd::SetReplaygain(model.settings.replaygain)),
        ]),
        Driver::Library => Cmd::Batch(vec![
            Effect::Library(LibraryCmd::LoadFavorites),
            Effect::Library(LibraryCmd::ScanLibrary {
                root: model.music_dir.clone(),
                revision: Revision::UNSTAMPED,
            }),
        ]),
        Driver::Config => {
            Effect::Config(ConfigCmd::SelectTheme(model.themes.selected.clone())).into()
        }
        Driver::Macos => Cmd::Batch(vec![
            Effect::System(SystemCmd::NowPlaying(NowPlaying::default())),
            Effect::System(SystemCmd::PlaybackState(Playback::Paused)),
            Effect::System(SystemCmd::Volume(model.transport.volume)),
        ]),
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
        Effect::Audio(AudioCmd::Pause(playback)),
    ])
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::{DriverFailure, DriverStatus},
        message::DriverMessage,
        update::{
            driver::{DriverRejection, DriverSignal},
            machine::Machine,
        },
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
        outcome: Result<Option<DriverSignal>, DriverRejection>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        start: DriverStatus::Running,
        message: died(),
        next: dead(),
        outcome: Ok(Some(DriverSignal::Died(DriverFailure::Panicked("boom".to_string())))),
    })]
    #[case::running_stops(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Stopped,
        next: DriverStatus::Stopped,
        outcome: Ok(None),
    })]
    #[case::running_is_congested(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Congested,
        next: DriverStatus::Running,
        outcome: Ok(Some(DriverSignal::Congested)),
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
    #[case::dead_refuses_congestion(LifeRow {
        start: dead(),
        message: DriverMessage::Congested,
        next: dead(),
        outcome: Err(DriverRejection::Dead),
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
    #[case::stopped_refuses_congestion(LifeRow {
        start: DriverStatus::Stopped,
        message: DriverMessage::Congested,
        next: DriverStatus::Stopped,
        outcome: Err(DriverRejection::Stopped),
    })]
    #[case::running_reports_a_rejected_input(LifeRow {
        start: DriverStatus::Running,
        message: DriverMessage::Rejected { input: "seek" },
        next: DriverStatus::Running,
        outcome: Err(DriverRejection::Input("seek")),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.start;
        let outcome = status.update(row.message);
        assert_eq!(status, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
