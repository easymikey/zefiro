use crate::domain::{DeleteCandidate, Workspace, playlist::Playlist};

pub(crate) fn candidate(
    playlist: &Playlist,
    workspace: &Workspace,
) -> Option<DeleteCandidate> {
    let track = playlist.tracks.get(workspace.browse.selected().get())?;
    Some(DeleteCandidate {
        track: workspace.browse.selected(),
        title: track.song_title(),
        artist: track.tags().artist.clone().unwrap_or_default(),
    })
}
