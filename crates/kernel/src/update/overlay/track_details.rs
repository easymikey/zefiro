use std::sync::Arc;

use crate::domain::{
    player::Player,
    playlist::Playlist,
    track::Track,
    workspace::Workspace,
};

pub(crate) fn candidate(
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
