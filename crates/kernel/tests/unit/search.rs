use std::sync::Arc;

use kernel::{Track, domain::Tags, search};
use rstest::rstest;

use crate::support::track_with_tags;

fn tagged(title: Option<&str>, artist: Option<&str>, album: Option<&str>) -> Tags {
    Tags {
        title: title.map(str::to_string),
        artist: artist.map(str::to_string),
        album: album.map(str::to_string),
        ..Default::default()
    }
}

fn moon_and_sun() -> Vec<Arc<Track>> {
    vec![
        track_with_tags(
            "/m/one.flac",
            tagged(Some("Moon River"), Some("A"), Some("X")),
        ),
        track_with_tags(
            "/m/two.flac",
            tagged(Some("Sun Song"), Some("B"), Some("Y")),
        ),
    ]
}

fn moon_sun_moonlight() -> Vec<Arc<Track>> {
    let mut tracks = moon_and_sun();
    tracks.push(track_with_tags(
        "/m/three.flac",
        tagged(Some("Moonlight Sonata"), Some("C"), Some("Z")),
    ));
    tracks
}

fn scattered_and_exact() -> Vec<Arc<Track>> {
    vec![
        track_with_tags("/m/scattered.flac", tagged(None, Some("xmxoxoxn"), None)),
        track_with_tags("/m/exact.flac", tagged(Some("Moon"), None, None)),
    ]
}

fn alpha_beta_special() -> Vec<Arc<Track>> {
    vec![
        track_with_tags("/m/a.flac", tagged(Some("Alpha"), Some("Zeta"), None)),
        track_with_tags("/m/b.flac", tagged(Some("Beta"), None, Some("Gamma"))),
        track_with_tags("/special-file.flac", Tags::default()),
    ]
}

#[rstest]
#[case::empty_query_returns_every_index_in_playlist_order(moon_and_sun(), "", vec![0, 1])]
#[case::narrows_to_only_the_matching_indices(moon_sun_moonlight(), "moon", vec![0, 2])]
#[case::orders_best_match_first_regardless_of_playlist_position(
    scattered_and_exact(),
    "moon",
    vec![1, 0]
)]
#[case::matches_artist_not_just_title(alpha_beta_special(), "zeta", vec![0])]
#[case::matches_album_not_just_title(alpha_beta_special(), "gamma", vec![1])]
#[case::matches_filename_not_just_title(alpha_beta_special(), "special", vec![2])]
fn rank_orders_and_filters_tracks(
    #[case] tracks: Vec<Arc<Track>>,
    #[case] query: &str,
    #[case] expected: Vec<usize>,
) {
    assert_eq!(
        search::rank(&tracks, query)
            .into_iter()
            .map(usize::from)
            .collect::<Vec<_>>(),
        expected
    );
}
