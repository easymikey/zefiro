use kernel::domain::{index::TrackIndex, playlist};

fn main() {
    let mut list = playlist::Playlist::from_tracks(Vec::new());
    assert!(list.jump(TrackIndex::new(0)).is_none());
}
