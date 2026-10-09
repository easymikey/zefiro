use kernel::domain::{
    appearance::{Breakpoints, CoverMode, KeyHints, LayoutMode},
    catalog::{BrowseLevel, Catalog, CatalogName},
    geometry::Cells,
    server::{
        Account,
        AlbumOrder,
        Endpoint,
        Listing,
        PlaylistId,
        Server,
        ServerName,
        ServerStatus,
        UserName,
    },
};
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::{
    scene::{PixelPath, Scene},
    screen::{frame_layout::FrameLayout, root::ScreenWidget},
};

use crate::support::{
    fixtures::{SceneSources, model_with_tracks, rendered},
    playing_track,
};

fn painted_frame(scene: Scene<'_>, size: (u16, u16)) -> (FrameLayout<'_>, String) {
    let (width, height) = size;
    let layout = FrameLayout::from_scene(&scene, Rect::new(0, 0, width, height));
    let text = rendered(width, height, |frame| {
        frame.render_widget(&ScreenWidget::new(scene, &layout), frame.area());
    })
    .to_string();
    (layout, text)
}

fn frame(scene: Scene<'_>, size: (u16, u16)) -> String {
    painted_frame(scene, size).1
}

fn tiny_breakpoints() -> Breakpoints {
    Breakpoints {
        min_width: Cells(20),
        min_height: Cells(3),
        ..Breakpoints::default()
    }
}

#[test]
fn narrowing_one_column_below_full_switches_from_the_card_to_the_compact_arrangement() {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.pixel_path = PixelPath::Protocol;
    let scene = sources.scene();
    let breakpoints = Breakpoints::default();
    let wide = frame(
        scene,
        (breakpoints.full_min_width.0, breakpoints.full_min_height.0),
    );
    let narrow = frame(
        scene,
        (
            breakpoints.full_min_width.0 - 1,
            breakpoints.full_min_height.0,
        ),
    );
    assert!(wide.contains("No cover"), "got {wide:?}");
    assert!(!narrow.contains("No cover"), "got {narrow:?}");
}

#[test]
fn narrowing_one_column_below_compact_drops_the_playlist_pane_entirely() {
    let mut sources = SceneSources::new(playing_track("Boundary Song"));
    sources.appearance_mut().breakpoints = tiny_breakpoints();
    let breakpoints = sources.appearance_mut().breakpoints;

    let (at_floor, at_floor_text) = painted_frame(
        sources.scene(),
        (
            breakpoints.compact_min_width.0,
            breakpoints.compact_min_height.0,
        ),
    );
    assert!(at_floor.playlist_areas.is_some(), "got {at_floor_text:?}");

    let (narrow, narrow_text) = painted_frame(
        sources.scene(),
        (
            breakpoints.compact_min_width.0 - 1,
            breakpoints.compact_min_height.0,
        ),
    );
    assert!(narrow.playlist_areas.is_none(), "got {narrow_text:?}");
    assert!(
        narrow_text.contains("Boundary Song"),
        "minimal must still show the track title, got:\n{narrow_text}"
    );
}

#[test]
fn a_vinyl_cover_at_the_full_floor_still_leaves_the_title_visible() {
    let mut sources = SceneSources::new(playing_track("Vinyl Floor Song"));
    sources.model.settings.appearance_settings.cover_mode = CoverMode::Vinyl;
    sources.pixel_path = PixelPath::Protocol;
    let scene = sources.scene();
    let breakpoints = Breakpoints::default();
    let text = frame(
        scene,
        (breakpoints.full_min_width.0, breakpoints.full_min_height.0),
    );
    assert!(text.contains("Vinyl Floor Song"), "got {text:?}");
}

fn albums_top() -> Catalog {
    Catalog {
        albums_level: BrowseLevel::new(Listing::Albums(AlbumOrder::Newest)),
        ..Catalog::new(ServerName::new("home"))
    }
}

fn inside_a_playlist() -> Catalog {
    Catalog {
        albums_level: BrowseLevel::new(Listing::Playlists),
        album_level: Some(BrowseLevel::new(Listing::Playlist(PlaylistId::new("pl-0")))),
        ..Catalog::new(ServerName::new("home"))
    }
}

#[rstest]
#[case::local_with_a_server("local_with_a_server", CatalogName::Local, None)]
#[case::server_songs_top(
    "server_songs_top",
    CatalogName::Server(ServerName::new("home")),
    Some(Catalog::new(ServerName::new("home")))
)]
#[case::server_albums_top(
    "server_albums_top",
    CatalogName::Server(ServerName::new("home")),
    Some(albums_top())
)]
#[case::server_inside_a_playlist(
    "server_inside_a_playlist",
    CatalogName::Server(ServerName::new("home")),
    Some(inside_a_playlist())
)]
fn the_key_hints_name_the_keys_of_the_tab(
    #[case] suffix: &str,
    #[case] catalog_name: CatalogName,
    #[case] catalog: Option<Catalog>,
) {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.servers = vec![Server {
        account: Account {
            server_name: ServerName::new("home"),
            endpoint: Endpoint::parse("https://music.example").unwrap(),
            user_name: UserName::new("mikey").unwrap(),
        },
        server_status: ServerStatus::Connecting,
    }];
    sources.model.catalog_name = catalog_name;
    sources.model.catalogs = catalog.into_iter().collect();
    let text = frame(sources.scene(), (120, 40));
    let hints = text
        .lines()
        .filter(|line| line.contains("Help"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::with_settings!({ snapshot_suffix => suffix }, {
        insta::assert_snapshot!(hints);
    });
}

#[rstest]
fn the_key_hints_and_layout_rows_shape_the_whole_frame(
    #[values(KeyHints::Shown, KeyHints::Hidden)] key_hints: KeyHints,
    #[values(LayoutMode::Auto, LayoutMode::Compact)] layout_mode: LayoutMode,
) {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.settings.appearance_settings.key_hints = key_hints;
    sources.model.settings.appearance_settings.layout_mode = layout_mode;
    let text = frame(sources.scene(), (120, 40));
    insta::with_settings!({
        snapshot_suffix => format!("{key_hints:?}_{layout_mode:?}"),
    }, {
        insta::assert_snapshot!(text);
    });
}
