use crate::domain::{
    overlay::DeleteCandidate,
    playlist::Playlist,
    workspace::Workspace,
};

pub(crate) fn candidate(
    playlist: &Playlist,
    workspace: &Workspace,
) -> Option<DeleteCandidate> {
    let track = playlist.tracks.get(workspace.browse.selected().get())?;
    Some(DeleteCandidate {
        source: track.source().clone(),
        title: track.song_title(),
        artist: track.tags().artist.clone().unwrap_or_else(String::new),
    })
}
