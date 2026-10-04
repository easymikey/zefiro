use std::{path::PathBuf, time::Duration};

use kernel::Playback;

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
            }) => Some(current),
            Phase::Idle
            | Phase::Loading(_)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(_),
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

    pub(crate) fn promote(&mut self) -> bool {
        let Next::Crossfading { preload, .. } = &mut self.next else {
            return false;
        };
        std::mem::swap(&mut self.current, preload);
        self.next = Next::None;
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Handover {
    pub(crate) incoming: Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    Loading(Loading),
    Playing(CurrentTrack),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurrentTrack {
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
        preload: CurrentTrack,
        fade: Fade,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fade {
    Armed,
    Running,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loading {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) after_load: Option<Resume>,
}

impl Loading {
    pub(crate) fn into_current(
        self,
        decoded: Option<Duration>,
    ) -> (CurrentTrack, Option<Resume>) {
        let Loading {
            path,
            gain,
            after_load,
        } = self;
        let total = after_load.as_ref().map_or(decoded, |resume| resume.total);
        (CurrentTrack { total, gain, path }, after_load)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resume {
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) total: Option<Duration>,
}
