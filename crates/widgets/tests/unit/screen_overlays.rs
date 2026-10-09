use kernel::domain::{
    cursor_over::CursorOver,
    overlay::{Overlay, SearchQuery, TextEntry, Verdict},
    revision::Revision,
    setting_row::SettingRow,
};
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::screen::{frame_layout::FrameLayout, root::ScreenWidget};

use crate::support::fixtures::{SceneSources, model_with_tracks, rendered, track};

fn frame_with_overlay(overlay: Overlay) -> String {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.workspace.overlay = Some(overlay);
    let scene = sources.scene();
    let area = Rect::new(0, 0, 80, 24);
    let layout = FrameLayout::from_scene(&scene, area);
    assert!(
        layout.overlay_areas.is_some(),
        "an active overlay must claim a rect in the full frame's layout"
    );
    rendered(80, 24, |frame| {
        frame.render_widget(&ScreenWidget::new(scene, &layout), frame.area());
    })
    .to_string()
}

#[rstest]
#[case::help(Overlay::Help, "KEYS")]
#[case::search(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)), "SEARCH")]
#[case::history(Overlay::History(CursorOver::new((), 0)), "HISTORY")]
#[case::settings(Overlay::Settings(SettingRow::Theme), "SETTINGS")]
#[case::confirm_trash(Overlay::ConfirmTrash(track("Moon River")), "MOVE TO TRASH?")]
#[case::jump_to_time(Overlay::JumpToTime(TextEntry::default()), "JUMP TO TIME")]
#[case::track_details(Overlay::TrackDetails(track("Moon River")), "TRACK INFO")]
#[case::music_dir(Overlay::MusicDir { text_entry: TextEntry::default(), verdict: None, revision: None }, "LIBRARY FOLDER")]
fn an_overlay_is_painted_over_the_full_frame(
    #[case] overlay: Overlay,
    #[case] title: &str,
) {
    let text = frame_with_overlay(overlay);
    assert!(text.contains(title), "got {text:?}");
}

#[test]
fn the_music_folder_hint_offers_check_while_a_check_runs() {
    let text = frame_with_overlay(Overlay::MusicDir {
        text_entry: TextEntry {
            input: "/Users/me/Music".to_string(),
            error: None,
        },
        verdict: Some(Verdict::Readable),
        revision: Some(Revision::default()),
    });
    assert!(
        text.contains("Enter check") && !text.contains("Enter save"),
        "got {text:?}"
    );
}

#[test]
fn the_save_playlist_banner_is_painted_over_the_full_frame() {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.workspace.overlay = Some(Overlay::SavePlaylist(TextEntry {
        input: "mixtape".to_string(),
        error: None,
    }));
    let scene = sources.scene();
    let area = Rect::new(0, 0, 80, 24);
    let layout = FrameLayout::from_scene(&scene, area);
    let text = rendered(80, 24, |frame| {
        frame.render_widget(&ScreenWidget::new(scene, &layout), frame.area());
    })
    .to_string();
    assert!(text.contains("mixtape"), "got {text:?}");
    assert!(
        text.contains("song00"),
        "the frame behind the banner must still be there, got {text:?}"
    );
}
