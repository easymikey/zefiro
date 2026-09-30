use kernel::{domain::TrackIndex, playlist};

fn main() {
    let mut list = playlist::Playlist::from_tracks(Vec::new());
    let _ = list.jump(TrackIndex::new(0));
}
