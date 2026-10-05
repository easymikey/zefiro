use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::domain::{
    favorites::Favorites,
    library,
    library::SortKey,
    track::{AudioFormat, Tags, Track},
};
use rstest::rstest;

fn track(path: &str, artist: Option<&str>, album: Option<&str>) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(path)
            .duration(Duration::from_secs(1))
            .tags(Tags {
                artist: artist.map(str::to_string),
                album: album.map(str::to_string),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
}

fn favorited(paths: &[&str]) -> Favorites {
    let mut favorites = Favorites::default();
    for path in paths {
        favorites.toggle(kernel::domain::track::TrackRef::Local(PathBuf::from(*path)));
    }
    favorites
}

struct SortRow {
    tracks: Vec<Arc<Track>>,
    key: SortKey,
    favorites: Favorites,
    expected: Vec<usize>,
}

#[rstest]
#[case::by_artist_then_album(SortRow {
    tracks: vec![
        track("/t0.flac", Some("Björk"), Some("Vespertine")),
        track("/t1.flac", Some("ABBA"), Some("Voulez-Vous")),
        track("/t2.flac", Some("Air"), Some("Moon Safari")),
        track("/t3.flac", Some("abba"), Some("Arrival")),
    ],
    key: SortKey::Artist,
    favorites: Favorites::default(),
    expected: vec![3, 1, 2, 0],
})]
#[case::by_favorites_puts_favorited_first_and_keeps_relative_order(SortRow {
    tracks: vec![
        track("/t0.flac", None, None),
        track("/t1.flac", None, None),
        track("/t2.flac", None, None),
    ],
    key: SortKey::Favorites,
    favorites: favorited(&["/t2.flac"]),
    expected: vec![2, 0, 1],
})]
#[case::by_favorites_with_no_favorites_keeps_added_order(SortRow {
    tracks: vec![track("/t0.flac", None, None), track("/t1.flac", None, None)],
    key: SortKey::Favorites,
    favorites: Favorites::default(),
    expected: vec![0, 1],
})]
fn sort_indices_orders_tracks(#[case] case: SortRow) {
    let result = library::sort_indices(&case.tracks, case.key, &case.favorites);
    assert_eq!(
        result.into_iter().map(usize::from).collect::<Vec<_>>(),
        case.expected
    );
}
