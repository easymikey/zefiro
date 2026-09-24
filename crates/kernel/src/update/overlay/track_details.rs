use std::sync::Arc;

use crate::domain::{Player, Track, Workspace, playlist::Playlist};

pub(super) fn candidate(
    playlist: &Playlist,
    player: &Player,
    workspace: &Workspace,
) -> Option<Arc<Track>> {
    playlist
        .tracks
        .get(workspace.browse.selected().get())
        .cloned()
        .or_else(|| player.current().cloned())
}
