use kernel::domain::{index::TrackIndex, playlist};

fn main() {
    let mut playlist = playlist::Playlist::from_tracks(Vec::new());
    assert!(playlist.jump(TrackIndex::new(0)).is_none());
}
