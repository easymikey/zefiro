use std::time::Duration;

use kernel::{
    cmd::{Media, Playback},
    domain::track::Decibels,
};

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
            Phase::Playing(Playing {
                current,
                next: _next,
            }) => Some(current),
            Phase::Handover(Incoming::Playing(current)) => Some(current),
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

    pub(crate) fn promote(&mut self) {
        if let NextTrack::Crossfading {
            incoming,
            fade: _fade,
        } = &mut self.next
        {
            std::mem::swap(&mut self.current, incoming);
            self.next = NextTrack::None;
        }
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
    pub(crate) decibels: Option<Decibels>,
    pub(crate) media: Media,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum NextTrack {
    #[default]
    None,
    Preloading {
        media: Media,
        decibels: Option<Decibels>,
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
    pub(crate) media: Media,
    pub(crate) decibels: Option<Decibels>,
    pub(crate) resume: Option<Resume>,
}

impl Loading {
    pub(crate) fn into_current(
        self,
        duration: Option<Duration>,
    ) -> (LoadedTrack, Option<Resume>) {
        let Loading {
            media,
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
                media,
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
    pub(crate) upcoming: Option<Upcoming>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Upcoming {
    pub(crate) media: Media,
    pub(crate) decibels: Option<Decibels>,
}

impl From<LoadedTrack> for Upcoming {
    fn from(track: LoadedTrack) -> Self {
        let LoadedTrack {
            duration: _duration,
            decibels,
            media,
        } = track;
        Upcoming { media, decibels }
    }
}
