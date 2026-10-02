use config::{CoverStyle, KeyHints, LayoutMode};
use kernel::domain::appearance::Breakpoints;
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::{CoverArt, FrameLayout, PixelPath, Scene, Screen};

use crate::unit::support::{SceneSources, model_with_tracks, playing_track, rendered};

fn painted_frame(scene: Scene<'_>, size: (u16, u16)) -> (FrameLayout, String) {
    let (width, height) = size;
    let layout =
        FrameLayout::new(&scene.layout_parts(), Rect::new(0, 0, width, height));
    let text = rendered(width, height, |frame| {
        frame.render_widget(
            &Screen {
                scene,
                layout: &layout,
                cover_art: &CoverArt::Missing,
            },
            frame.area(),
        );
    })
    .to_string();
    (layout, text)
}

fn frame(scene: Scene<'_>, size: (u16, u16)) -> String {
    painted_frame(scene, size).1
}

fn tiny_breakpoints() -> Breakpoints {
    Breakpoints {
        min_columns: 20,
        min_rows: 3,
        ..Breakpoints::default()
    }
}

#[test]
fn full_layout_at_a_large_terminal_shows_the_cover_and_the_playlist() {
    let sources = SceneSources::new(model_with_tracks(3));
    let scene = Scene {
        pixel_path: PixelPath::Protocol,
        ..sources.scene()
    };
    let text = frame(scene, (120, 40));
    assert!(text.contains("No cover"), "got {text:?}");
    assert!(text.contains("song00"), "got {text:?}");
}

#[test]
fn compact_layout_at_a_small_terminal_hides_the_cover_and_shows_the_playlist() {
    let mut sources = SceneSources::new(playing_track("Test Song"));
    sources.look_mut().breakpoints = tiny_breakpoints();
    let bp = sources.look_mut().breakpoints;
    let text = frame(
        sources.scene(),
        (bp.compact_min_width, bp.compact_min_height),
    );
    assert!(!text.contains("No cover"), "got {text:?}");
    assert!(text.contains("Test Song"), "got {text:?}");
}

#[test]
fn minimal_layout_renders_all_three_rows_when_height_allows() {
    let mut sources = SceneSources::new(playing_track("Test Song"));
    sources.look_mut().breakpoints = tiny_breakpoints();
    let text = frame(sources.scene(), (25, 3));
    assert!(text.contains("Test Song"), "got {text:?}");
}

#[test]
fn one_row_terminal_shows_the_too_small_message_instead_of_a_degraded_minimal_row() {
    let mut sources = SceneSources::new(playing_track("Test Song"));
    sources.look_mut().breakpoints = tiny_breakpoints();
    let text = frame(sources.scene(), (25, 1));
    assert!(text.contains("Terminal too small."), "got {text:?}");
    assert!(!text.contains("Test Song"), "got {text:?}");
}

#[test]
fn narrowing_one_column_below_full_switches_from_the_card_to_the_compact_arrangement() {
    let sources = SceneSources::new(model_with_tracks(3));
    let scene = Scene {
        pixel_path: PixelPath::Protocol,
        ..sources.scene()
    };
    let bp = Breakpoints::default();
    let wide = frame(scene, (bp.full_min_width, bp.full_min_height));
    let narrow = frame(scene, (bp.full_min_width - 1, bp.full_min_height));
    assert!(wide.contains("No cover"), "got {wide:?}");
    assert!(!narrow.contains("No cover"), "got {narrow:?}");
}

#[test]
fn narrowing_one_column_below_compact_drops_the_playlist_pane_entirely() {
    let mut sources = SceneSources::new(playing_track("Boundary Song"));
    sources.look_mut().breakpoints = tiny_breakpoints();
    let bp = sources.look_mut().breakpoints;

    let (at_floor, at_floor_text) = painted_frame(
        sources.scene(),
        (bp.compact_min_width, bp.compact_min_height),
    );
    assert!(at_floor.playlist.is_some(), "got {at_floor_text:?}");

    let (narrow, narrow_text) = painted_frame(
        sources.scene(),
        (bp.compact_min_width - 1, bp.compact_min_height),
    );
    assert!(narrow.playlist.is_none(), "got {narrow_text:?}");
    assert!(
        narrow_text.contains("Boundary Song"),
        "minimal must still show the track title, got:\n{narrow_text}"
    );
}

#[test]
fn a_vinyl_cover_at_the_full_floor_still_leaves_the_title_visible() {
    let mut sources = SceneSources::new(playing_track("Vinyl Floor Song"));
    sources.look_mut().appearance.cover_style = CoverStyle::Vinyl;
    let scene = Scene {
        pixel_path: PixelPath::Protocol,
        ..sources.scene()
    };
    let bp = Breakpoints::default();
    let text = frame(scene, (bp.full_min_width, bp.full_min_height));
    assert!(text.contains("Vinyl Floor Song"), "got {text:?}");
}

#[rstest]
fn the_key_hints_and_layout_rows_shape_the_whole_frame(
    #[values(KeyHints::Shown, KeyHints::Hidden)] key_hints: KeyHints,
    #[values(LayoutMode::Auto, LayoutMode::Compact)] mode: LayoutMode,
) {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.look_mut().appearance.key_hints = key_hints;
    sources.look_mut().appearance.layout_mode = mode;
    let text = frame(sources.scene(), (120, 40));
    insta::with_settings!({
        snapshot_suffix => format!("{key_hints:?}_{mode:?}"),
    }, {
        insta::assert_snapshot!(text);
    });
}
