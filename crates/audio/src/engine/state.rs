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
        message::DeviceChoice,
        phase::Phase,
        revisions::JobRevisions,
    },
    gain::Gain,
};

#[derive(Debug)]
pub(crate) struct Engine {
    pub(crate) state: EngineState,
    pub(crate) job_revisions: JobRevisions,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EngineState {
    Closed(Closed),
    Live(Live),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Closed {
    pub(crate) settings: AudioSettings,
    pub(crate) pending: Option<TrackLoad>,
    pub(crate) speed: Speed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub(crate) phase: Phase,
    pub(crate) speed: Speed,
    pub(crate) settings: AudioSettings,
    pub(crate) executed: ExecutedRevisions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExecutedRevisions {
    pub(crate) load: Revision,
    pub(crate) incoming: Revision,
}

impl Live {
    #[must_use]
    pub(crate) fn new(settings: AudioSettings, speed: Speed) -> Self {
        Self {
            phase: Phase::Idle,
            speed,
            settings,
            executed: ExecutedRevisions::default(),
        }
    }

    pub(crate) fn gain(&self) -> Gain {
        let gain = self.phase.current().and_then(|current| current.gain);
        replay_gain_factor(self.settings.replay_gain, gain)
    }
}

pub(crate) fn announce(
    opened: DeviceChoice,
    device: OutputDevice,
    loop_cmd: AudioLoopCmd,
) -> AudioLoopCmd {
    if matches!(opened, DeviceChoice::Requested) {
        return loop_cmd;
    }
    Cmd::message(AudioEvent::DeviceFellBack(device)).then(loop_cmd)
}

pub(crate) fn then_report(loop_cmd: AudioLoopCmd) -> AudioLoopCmd {
    loop_cmd.then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
}
