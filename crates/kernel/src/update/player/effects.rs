use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, LibraryCmd, MacosCmd},
    domain::{
        cue::{Cue, PlaybackChange},
        history::HistoryEntry,
        time::Moment,
        track::Track,
    },
};

pub(crate) fn seek_effect(target: Duration) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Seek(target)),
        Effect::Macos(MacosCmd::SetPosition(target)),
    ])
}

pub(crate) fn handover_effects(
    track: &Arc<Track>,
    playback: PlaybackChange,
    now: Moment,
) -> Vec<Effect> {
    [
        Effect::Library(LibraryCmd::AppendHistory(HistoryEntry::from_track(
            track, now,
        ))),
        Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track)))),
    ]
    .into_iter()
    .chain(playback.effects())
    .chain([
        Effect::Animate(Cue::TrackChanged),
        Effect::Animate(Cue::PlaybackChanged(playback)),
    ])
    .collect()
}
