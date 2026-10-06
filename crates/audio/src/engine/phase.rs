use std::{path::PathBuf, time::Duration};

use kernel::cmd::Playback;

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Loading(Loading),
    Playing(Playing),
    Handover(Incoming),
}

impl Phase {
    pub(crate) fn current(&self) -> Option<&LoadedTrack> {
        match self {
            Phase::Playing(Playing { current, .. })
            | Phase::Handover(Incoming::Playing(current)) => Some(current),
            Phase::Idle | Phase::Loading(_) | Phase::Handover(Incoming::Loading(_)) => {
                None
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Playing {
    pub(crate) current: LoadedTrack,
    pub(crate) next: NextTrack,
}

impl Playing {
    #[must_use]
    pub(crate) fn new(current: LoadedTrack) -> Self {
        Self {
            current,
            next: NextTrack::None,
        }
    }

    pub(crate) fn promote(&mut self) -> bool {
        let NextTrack::Crossfading { incoming, .. } = &mut self.next else {
            return false;
        };
        std::mem::swap(&mut self.current, incoming);
        self.next = NextTrack::None;
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    Loading(Loading),
    Playing(LoadedTrack),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadedTrack {
    pub(crate) duration: Option<Duration>,
    pub(crate) decibels: Option<kernel::domain::track::Decibels>,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum NextTrack {
    #[default]
    None,
    Preloading {
        path: PathBuf,
        decibels: Option<kernel::domain::track::Decibels>,
    },
    Gapless(LoadedTrack),
    Crossfading {
        incoming: LoadedTrack,
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
    pub(crate) decibels: Option<kernel::domain::track::Decibels>,
    pub(crate) resume: Option<Resume>,
}

impl Loading {
    pub(crate) fn into_current(
        self,
        duration: Option<Duration>,
    ) -> (LoadedTrack, Option<Resume>) {
        let Loading {
            path,
            decibels,
            resume: after_load,
        } = self;
        let duration = after_load
            .as_ref()
            .map_or(duration, |resume| resume.duration);
        (
            LoadedTrack {
                duration,
                decibels,
                path,
            },
            after_load,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resume {
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) duration: Option<Duration>,
}
