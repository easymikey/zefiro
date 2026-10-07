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
        index: ViewIndex,
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
            | Successor::Following(track) => Some(track),
            Successor::Queued {
                track,
                queue_index: _queue_index,
                index: _index,
            } => Some(track),
            Successor::Nothing => None,
        }
    }

    pub(crate) fn into_track(self) -> Option<Arc<Track>> {
        match self {
            Successor::Preloaded(track)
            | Successor::Repeating(track)
            | Successor::Following(track) => Some(track),
            Successor::Queued {
                track,
                queue_index: _queue_index,
                index: _index,
            } => Some(track),
            Successor::Nothing => None,
        }
    }
}

pub(crate) fn successor(playlist: &Playlist, queue: &[TrackSource]) -> Successor {
    if matches!(playlist.repeat_mode, RepeatMode::One) {
        return playlist
            .current()
            .cloned()
            .map_or(Successor::Nothing, Successor::Repeating);
    }
    if let Some((queue_index, index, track)) = first_queued(playlist, queue) {
        return Successor::Queued {
            queue_index,
            index,
            track: Arc::clone(track),
        };
    }
    playlist
        .upcoming()
        .cloned()
        .map_or(Successor::Nothing, Successor::Following)
}

pub(crate) fn first_queued<'a>(
    playlist: &'a Playlist,
    queue: &[TrackSource],
) -> Option<(usize, ViewIndex, &'a Arc<Track>)> {
    queue.iter().enumerate().find_map(|(queue_index, source)| {
        let index = index_of(&playlist.tracks, source)?;
        let track = playlist.tracks.get(index)?;
        Some((queue_index, ViewIndex::new(index), track))
    })
}
