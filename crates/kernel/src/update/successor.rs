use std::sync::Arc;

use crate::domain::{
    index::ViewIndex,
    playlist::{Playlist, RepeatMode, index_of},
    track::{Track, TrackRef},
};

pub(crate) enum Successor {
    Preloaded(Arc<Track>),
    Repeating(Arc<Track>),
    Queued {
        position: usize,
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
            | Successor::Queued { track, .. }
            | Successor::Following(track) => Some(track),
            Successor::Nothing => None,
        }
    }
}

pub(crate) fn successor(playlist: &Playlist, queue: &[TrackRef]) -> Successor {
    if matches!(playlist.repeat, RepeatMode::One) {
        return playlist
            .current()
            .cloned()
            .map_or(Successor::Nothing, Successor::Repeating);
    }
    if let Some((position, index, track)) = first_queued(playlist, queue) {
        return Successor::Queued {
            position,
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
    queue: &[TrackRef],
) -> Option<(usize, ViewIndex, &'a Arc<Track>)> {
    queue.iter().enumerate().find_map(|(position, source)| {
        let index = index_of(&playlist.tracks, source)?;
        let track = playlist.tracks.get(index)?;
        Some((position, ViewIndex::new(index), track))
    })
}
