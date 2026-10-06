use kernel::{
    cmd::{Cmd, TrackLoad},
    domain::{
        device::OutputDevice,
        revision::Revision,
        settings::AudioSettings,
        speed::Speed,
    },
    message::AudioEvent,
    update::machine::LoopEffect,
};

use crate::{
    engine::{
        crossfade::replay_gain_factor,
        effect::{AudioLoopCmd, EngineEffect},
        message::DeviceOpened,
        phase::Phase,
        revisions::JobRevisions,
    },
    gain::Gain,
};

#[derive(Debug)]
pub(crate) struct Engine {
    pub(crate) state: EngineState,
    pub(crate) job_revisions: JobRevisions,
    pub(crate) device_choice: DeviceChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceChoice {
    FellBack,
    Requested,
}

impl Engine {
    #[must_use]
    pub(crate) fn new(state: EngineState) -> Self {
        Self {
            state,
            job_revisions: JobRevisions::default(),
            device_choice: DeviceChoice::Requested,
        }
    }

    pub(crate) fn opened(&mut self, device_opened: DeviceOpened) -> AudioLoopCmd {
        let device = device_opened.device.clone();
        let device_choice = self.device_choice;
        self.device_choice = DeviceChoice::Requested;
        announce(
            device_choice,
            device,
            self.state.opened(&mut self.job_revisions, device_opened),
        )
    }

    pub(crate) fn fell_back(&mut self) -> AudioLoopCmd {
        let speed = match &self.state {
            EngineState::Closed(closed) => closed.speed,
            EngineState::Live(live) => live.speed,
        };
        self.device_choice = DeviceChoice::FellBack;
        Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
            device: OutputDevice::SystemDefault,
            speed,
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EngineState {
    Closed(Closed),
    Live(Live),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Closed {
    pub(crate) settings: AudioSettings,
    pub(crate) track_load: Option<TrackLoad>,
    pub(crate) speed: Speed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub(crate) phase: Phase,
    pub(crate) speed: Speed,
    pub(crate) settings: AudioSettings,
    pub(crate) executed_revisions: ExecutedRevisions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExecutedRevisions {
    pub(crate) load: Revision,
    pub(crate) preload: Revision,
}

impl EngineState {
    pub(crate) fn opened(
        &mut self,
        job_revisions: &mut JobRevisions,
        device_opened: DeviceOpened,
    ) -> AudioLoopCmd {
        match std::mem::replace(self, EngineState::Closed(Closed::default())) {
            EngineState::Closed(closed) => {
                let (live, effect) = closed.reopened(job_revisions, device_opened);
                *self = EngineState::Live(live);
                effect
            }
            EngineState::Live(mut live) => {
                let effect = live.opened(job_revisions, device_opened);
                *self = EngineState::Live(live);
                effect
            }
        }
    }
}

impl Live {
    #[must_use]
    pub(crate) fn new(settings: AudioSettings, speed: Speed) -> Self {
        Self {
            phase: Phase::Idle,
            speed,
            settings,
            executed_revisions: ExecutedRevisions::default(),
        }
    }

    pub(crate) fn gain(&self) -> Gain {
        let gain = self.phase.current().and_then(|current| current.decibels);
        replay_gain_factor(self.settings.replay_gain, gain)
    }
}

pub(crate) fn announce(
    device_choice: DeviceChoice,
    device: OutputDevice,
    loop_cmd: AudioLoopCmd,
) -> AudioLoopCmd {
    if matches!(device_choice, DeviceChoice::Requested) {
        return loop_cmd;
    }
    Cmd::message(AudioEvent::DeviceFellBack(device)).then(loop_cmd)
}

pub(crate) fn then_report(loop_cmd: AudioLoopCmd) -> AudioLoopCmd {
    loop_cmd.then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
}
