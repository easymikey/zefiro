use std::sync::Arc;

use crate::domain::{
    index::ViewIndex,
    playlist::{Playlist, RepeatMode, index_of},
    track::{Track, TrackSource},
};

pub(crate) enum Successor {
    Preloaded(Arc<Track>),
    Repeating(Arc<Track>),
    Queued {
        queue_index: usize,
        index: Option<ViewIndex>,
        track: Arc<Track>,
    },
    Following(Arc<Track>),
    Nothing,
}

impl Successor {
    pub(crate) fn track(&self) -> Option<&Arc<Track>> {
        match self {
            Successor::Preloaded(track)
            | Successor::Repeating(track)
            | Successor::Following(track)
            | Successor::Queued { track, .. } => Some(track),
            Successor::Nothing => None,
        }
    }

    pub(crate) fn into_track(self) -> Option<Arc<Track>> {
        match self {
            Successor::Preloaded(track)
            | Successor::Repeating(track)
            | Successor::Following(track)
            | Successor::Queued { track, .. } => Some(track),
            Successor::Nothing => None,
        }
    }
}

pub(crate) fn successor(playlist: &Playlist, queue: &[Arc<Track>]) -> Successor {
    if matches!(playlist.repeat_mode, RepeatMode::One) {
        return playlist
            .current()
            .cloned()
            .map_or(Successor::Nothing, Successor::Repeating);
    }
    if let Some(queued) = first_queued(playlist, queue) {
        return queued;
    }
    playlist
        .upcoming()
        .cloned()
        .map_or(Successor::Nothing, Successor::Following)
}

pub(crate) fn first_queued(
    playlist: &Playlist,
    queue: &[Arc<Track>],
) -> Option<Successor> {
    queue.iter().enumerate().find_map(|(queue_index, queued)| {
        match index_of(&playlist.tracks, queued.source()) {
            Some(index) => Some(Successor::Queued {
                queue_index,
                index: Some(ViewIndex::new(index)),
                track: Arc::clone(playlist.tracks.get(index)?),
            }),
            None => match queued.source() {
                TrackSource::Server { .. } => Some(Successor::Queued {
                    queue_index,
                    index: None,
                    track: Arc::clone(queued),
                }),
                TrackSource::Local(_path) => None,
            },
        }
    })
}
