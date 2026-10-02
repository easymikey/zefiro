use kernel::{
    AudioError,
    AudioEvent,
    TrackLoad,
    domain::{OutputDevice, Revision, Speed},
};

use crate::{
    EngineConfig,
    deck::DeviceChoice,
    engine::{
        crossfade::replaygain_factor,
        effect::EngineEffect,
        phase::{Phase, Playing},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Engine {
    Muted(Muted),
    Live(Live),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Muted {
    pub(crate) error: AudioError,
    pub(crate) config: EngineConfig,
    pub(crate) pending: Option<TrackLoad>,
    pub(crate) speed: Speed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub(crate) phase: Phase,
    pub(crate) speed: Speed,
    pub(crate) config: EngineConfig,
    pub(crate) performed: PerformedRevisions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PerformedRevisions {
    pub(crate) load: Revision,
    pub(crate) incoming: Revision,
}

impl Live {
    #[must_use]
    pub(crate) fn new(config: EngineConfig, speed: Speed) -> Self {
        Self {
            phase: Phase::Idle,
            speed,
            config,
            performed: PerformedRevisions::default(),
        }
    }

    pub(crate) fn take_playing(&mut self) -> Option<Playing> {
        match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => Some(playing),
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                None
            }
        }
    }

    pub(crate) fn volume(&self) -> f32 {
        let gain = self.phase.current().and_then(|current| current.gain);
        replaygain_factor(self.config.replay_gain, gain)
    }
}

pub(crate) fn announce(
    opened: DeviceChoice,
    device: OutputDevice,
    effect: EngineEffect,
) -> EngineEffect {
    if matches!(opened, DeviceChoice::Requested) {
        return effect;
    }
    let told = EngineEffect::Send(AudioEvent::DeviceFellBack(device));
    if matches!(effect, EngineEffect::Nothing) {
        return told;
    }
    EngineEffect::Batch(vec![told, effect])
}

pub(crate) fn then_report(effect: EngineEffect) -> EngineEffect {
    if matches!(effect, EngineEffect::Nothing) {
        return EngineEffect::Report;
    }
    if let EngineEffect::Batch(mut steps) = effect {
        steps.push(EngineEffect::Report);
        return EngineEffect::Batch(steps);
    }
    EngineEffect::Batch(vec![effect, EngineEffect::Report])
}
