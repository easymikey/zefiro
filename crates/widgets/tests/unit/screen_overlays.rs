use kernel::domain::{
    cursor_over::CursorOver,
    overlay::{DeleteCandidate, JumpDigits, Overlay, SearchQuery, TextEntry},
    setting_row::SettingRow,
};
use ratatui::layout::Rect;
use widgets::{
    card::CardCover,
    screen::{frame_layout::FrameLayout, root::ScreenWidget},
};

use crate::unit::support::fixtures::{
    SceneSources,
    model_with_tracks,
    rendered,
    track,
};

fn frame_with_overlay(overlay: Overlay) -> String {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.workspace.overlay = Some(overlay);
    let scene = sources.scene();
    let area = Rect::new(0, 0, 80, 24);
    let layout = FrameLayout::from_scene(&scene, area);
    assert!(
        layout.overlay.is_some(),
        "an active overlay must claim a rect in the full frame's layout"
    );
    rendered(80, 24, |frame| {
        frame.render_widget(
            &ScreenWidget {
                scene,
                layout: &layout,
                cover_art: &CardCover::Missing,
            },
            frame.area(),
        );
    })
    .to_string()
}

#[test]
fn the_help_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::Help);
    assert!(text.contains("KEYS"), "got {text:?}");
}

#[test]
fn the_search_overlay_is_painted_over_the_full_frame() {
    let text =
        frame_with_overlay(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
    assert!(text.contains("SEARCH"), "got {text:?}");
}

#[test]
fn the_history_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::History(CursorOver::new((), 0)));
    assert!(text.contains("HISTORY"), "got {text:?}");
}

#[test]
fn the_settings_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::Settings(SettingRow::Theme));
    assert!(text.contains("SETTINGS"), "got {text:?}");
}

#[test]
fn the_confirm_delete_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::ConfirmDelete(DeleteCandidate {
        source: kernel::domain::track::TrackRef::Local("/music/moon.flac".into()),
        title: "Moon River".to_string(),
        artist: "Audrey Hepburn".to_string(),
    }));
    assert!(text.contains("MOVE TO TRASH?"), "got {text:?}");
}

#[test]
fn the_jump_to_time_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::JumpToTime(JumpDigits::default()));
    assert!(text.contains("JUMP TO TIME"), "got {text:?}");
}

#[test]
fn the_track_details_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::TrackDetails(track("Moon River")));
    assert!(text.contains("TRACK INFO"), "got {text:?}");
}

#[test]
fn the_source_dir_overlay_is_painted_over_the_full_frame() {
    let text = frame_with_overlay(Overlay::MusicDir {
        typed: TextEntry::default(),
        error: None,
    });
    assert!(text.contains("LIBRARY FOLDER"), "got {text:?}");
}

#[test]
fn the_save_playlist_banner_is_painted_over_the_full_frame() {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.workspace.overlay = Some(Overlay::SavePlaylist {
        typed: TextEntry {
            input: "mixtape".to_string(),
        },
        error: None,
    });
    let scene = sources.scene();
    let area = Rect::new(0, 0, 80, 24);
    let layout = FrameLayout::from_scene(&scene, area);
    let text = rendered(80, 24, |frame| {
        frame.render_widget(
            &ScreenWidget {
                scene,
                layout: &layout,
                cover_art: &CardCover::Missing,
            },
            frame.area(),
        );
    })
    .to_string();
    assert!(text.contains("mixtape"), "got {text:?}");
    assert!(
        text.contains("song00"),
        "the frame behind the banner must still be there, got {text:?}"
    );
}
