use kernel::{
    cmd::{Cmd, TrackLoad},
    domain::{
        device::OutputDevice,
        revision::Revision,
        settings::AudioSettings,
        speed::Speed,
    },
    message::AudioEvent,
};

use crate::{
    deck::DeviceChoice,
    engine::{crossfade::replay_gain_factor, effect::EngineEffect, phase::Phase},
    gain::Gain,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Engine {
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
    pub(crate) performed: PerformedRevisions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PerformedRevisions {
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
            performed: PerformedRevisions::default(),
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
    cmd: Cmd<EngineEffect, AudioEvent>,
) -> Cmd<EngineEffect, AudioEvent> {
    if matches!(opened, DeviceChoice::Requested) {
        return cmd;
    }
    Cmd::message(AudioEvent::DeviceFellBack(device)).then(cmd)
}

pub(crate) fn then_report(
    cmd: Cmd<EngineEffect, AudioEvent>,
) -> Cmd<EngineEffect, AudioEvent> {
    cmd.then(Cmd::effect(EngineEffect::Report))
}
