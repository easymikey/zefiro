use std::{path::PathBuf, time::Duration};

use kernel::Playback;

use crate::engine::effect::PreloadedTrack;

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Loading(Loading),
    Playing(Playing),
    Handover(Handover),
}

impl Phase {
    pub(crate) fn current(&self) -> Option<&CurrentTrack> {
        match self {
            Phase::Playing(Playing { current, .. })
            | Phase::Handover(Handover {
                incoming: Incoming::Playing(current),
                ..
            }) => Some(current),
            Phase::Idle
            | Phase::Loading(_)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(_),
                ..
            }) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Playing {
    pub(crate) current: CurrentTrack,
    pub(crate) next: Next,
    pub(crate) preloading: Option<PathBuf>,
}

impl Playing {
    #[must_use]
    pub(crate) fn new(current: CurrentTrack) -> Self {
        Self {
            current,
            next: Next::None,
            preloading: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Handover {
    pub(crate) outgoing: Outgoing,
    pub(crate) incoming: Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    Loading(Loading),
    Playing(CurrentTrack),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Outgoing {
    pub(crate) from: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurrentTrack {
    pub(crate) total: Option<Duration>,
    pub(crate) gain: Option<f32>,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Next {
    #[default]
    None,
    Gapless(PathBuf),
    Crossfading {
        preload: PreloadedTrack,
        fade: Fade,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Fade {
    #[default]
    Idle,
    Fading,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loading {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) after_load: AfterLoad,
}

impl Loading {
    pub(crate) fn into_current(
        self,
        decoded: Option<Duration>,
    ) -> (CurrentTrack, AfterLoad) {
        let Loading {
            path,
            gain,
            after_load,
        } = self;
        let total = match &after_load {
            AfterLoad::None => decoded,
            AfterLoad::Resume { total, .. } => *total,
        };
        (CurrentTrack { total, gain, path }, after_load)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AfterLoad {
    None,
    Resume {
        position: Duration,
        playback: Playback,
        total: Option<Duration>,
    },
}
