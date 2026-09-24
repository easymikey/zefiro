use config::{KeyHints, LayoutMode};
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::FrameLayout;

use crate::unit::support::{Scenery, model_with_tracks};

fn playlist_rows(key_hints: KeyHints, mode: LayoutMode) -> u16 {
    let mut sources = Scenery::new(model_with_tracks(3));
    sources.appearance.window.key_hints = key_hints;
    sources.appearance.layout.mode = mode;
    let layout = FrameLayout::new(&sources.scene(), Rect::new(0, 0, 120, 40));
    layout.playlist.map_or(0, |areas| areas.rows.height)
}

#[rstest]
#[case::auto(LayoutMode::Auto)]
#[case::compact(LayoutMode::Compact)]
fn hiding_the_key_hints_gives_its_row_to_the_playlist(#[case] mode: LayoutMode) {
    let with_hints = playlist_rows(KeyHints::Shown, mode);
    let without_hints = playlist_rows(KeyHints::Hidden, mode);
    assert_eq!(without_hints, with_hints + 1);
}

#[test]
fn forcing_compact_drops_the_cover_card_and_frees_its_rows() {
    let full_rows = playlist_rows(KeyHints::Shown, LayoutMode::Auto);
    let compact_rows = playlist_rows(KeyHints::Shown, LayoutMode::Compact);
    assert!(
        compact_rows > full_rows,
        "forcing Compact must free the rows Full's cover card would have \
         spent, got compact={compact_rows} full={full_rows}"
    );
}
