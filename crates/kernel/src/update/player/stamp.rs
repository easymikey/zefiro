use crate::{
    cmd::{AudioCmd, Effect},
    domain::{
        revision::{Revision, Revisions},
        speed::Speed,
        time::Moment,
        transport::Transport,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub started_at: Moment,
    pub speed: Speed,
}

impl Anchor {
    pub(crate) fn at(transport: &Transport, now: Moment) -> Self {
        Anchor {
            started_at: now,
            speed: transport.speed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub anchor: Anchor,
    pub revision: Revision,
}

impl Stamp {
    pub(crate) fn pending(
        transport: &Transport,
        revisions: &Revisions,
        now: Moment,
    ) -> Self {
        Stamp {
            anchor: Anchor::at(transport, now),
            revision: revisions.effects.next(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StartOrigin {
    User(Stamp),
    TrackEnded(Stamp),
}

impl StartOrigin {
    pub(crate) fn stamp(self) -> Stamp {
        match self {
            StartOrigin::User(stamp) | StartOrigin::TrackEnded(stamp) => stamp,
        }
    }

    pub(crate) fn stop(self) -> Option<Effect> {
        match self {
            StartOrigin::User(_) => Some(Effect::Audio(AudioCmd::Stop)),
            StartOrigin::TrackEnded(_) => None,
        }
    }
}
