use kernel::domain::{
    cursor::Cursor,
    cursor_over::CursorOver,
    index::{TrackIndex, ViewIndex},
    library::Library,
    overlay::{
        Folders,
        Overlay,
        SearchQuery,
        Subfolder,
        Subfolders,
        TextEntry,
        Verdict,
    },
    playlist::{Playlist, PlaylistSource},
    revision::Revision,
    server::ServerName,
    setting_row::SettingRow,
};
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::{
    overlay::modal::placement::OverlayAreas,
    screen::{frame_layout::FrameLayout, root::ScreenWidget},
};

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
#[case::music_dir(Overlay::MusicDir { text_entry: TextEntry::default(), verdict: None, revision: None, folders: CursorOver::default() }, "LIBRARY FOLDER")]
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
        folders: CursorOver::default(),
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

fn browsing(
    verdict: Verdict,
    subfolders: Option<Subfolders>,
    matches: Vec<usize>,
) -> Overlay {
    Overlay::MusicDir {
        text_entry: TextEntry {
            input: "/Users/me/".to_string(),
            error: None,
        },
        verdict: Some(verdict),
        revision: None,
        folders: CursorOver {
            cursor: Cursor::new(matches.len()).step(1),
            content: Folders {
                path: "/Users/me".into(),
                subfolders,
                matches,
                revision: None,
            },
        },
    }
}

fn listing(verdict: Verdict, subfolders: Vec<Subfolder>) -> Option<Subfolders> {
    Some(Subfolders {
        path: "/Users/me".into(),
        verdict,
        subfolders,
    })
}

fn with_a_list() -> Overlay {
    let subfolders: Vec<Subfolder> = (0..10)
        .map(|index| match index % 3 {
            0 => Subfolder::Audio(format!("album {index}")),
            _ => Subfolder::Plain(format!("folder {index}")),
        })
        .collect();
    browsing(
        Verdict::Readable,
        listing(Verdict::Readable, subfolders),
        (0..10).collect(),
    )
}

fn with_an_empty_list() -> Overlay {
    browsing(
        Verdict::Readable,
        listing(Verdict::Readable, Vec::new()),
        Vec::new(),
    )
}

fn in_a_denied_folder() -> Overlay {
    browsing(
        Verdict::Readable,
        listing(Verdict::Denied, Vec::new()),
        Vec::new(),
    )
}

fn overlay_areas(overlay: Overlay) -> Option<OverlayAreas> {
    let mut sources = SceneSources::new(model_with_tracks(3));
    sources.model.workspace.overlay = Some(overlay);
    FrameLayout::from_scene(&sources.scene(), Rect::new(0, 0, 80, 24)).overlay_areas
}

#[test]
fn the_music_folder_prompt_lists_the_subfolders_and_marks_those_with_audio() {
    insta::assert_snapshot!(frame_with_overlay(with_a_list()));
}

#[test]
fn the_music_folder_prompt_keeps_its_rows_for_an_empty_list() {
    insta::assert_snapshot!(frame_with_overlay(with_an_empty_list()));
}

#[test]
fn the_music_folder_prompt_shows_the_verdict_in_place_of_the_list() {
    insta::assert_snapshot!(frame_with_overlay(in_a_denied_folder()));
}

#[test]
fn the_music_folder_prompt_keeps_its_size_whatever_it_lists() {
    let with_a_list = overlay_areas(with_a_list());
    assert!(with_a_list.is_some());
    assert_eq!(overlay_areas(with_an_empty_list()), with_a_list);
    assert_eq!(
        overlay_areas(browsing(Verdict::Readable, None, Vec::new())),
        with_a_list
    );
    assert_eq!(
        overlay_areas(in_a_denied_folder()).map(|areas| areas.outer().height),
        with_a_list.map(|areas| areas.outer().height)
    );
}

#[test]
fn the_listing_verdict_widens_the_music_folder_prompt() {
    let width = |overlay| overlay_areas(overlay).map(|areas| areas.outer().width);
    assert!(width(in_a_denied_folder()) > width(with_a_list()));
}

#[test]
fn a_folder_both_checks_deny_shows_its_verdict_once() {
    let text = frame_with_overlay(browsing(
        Verdict::Denied,
        listing(Verdict::Denied, Vec::new()),
        Vec::new(),
    ));
    assert_eq!(text.matches("no permission").count(), 1, "{text}");
}

#[test]
fn a_local_search_while_a_server_album_plays_lists_the_library_tracks() {
    let mut sources = SceneSources::new(model_with_tracks(0));
    sources.model.playlist =
        Playlist::from_tracks(vec![track("Gamma"), track("Delta")]);
    sources.model.playlist_source =
        PlaylistSource::Server(ServerName::new("navidrome"));
    sources.model.library = Some(Library {
        tracks: vec![track("Alpha"), track("Beta")],
        track_indexes: vec![TrackIndex::new(0), TrackIndex::new(1)],
    });
    sources.model.workspace.overlay = Some(Overlay::Search(CursorOver::new(
        SearchQuery {
            input: String::new(),
            matches: vec![ViewIndex::new(0), ViewIndex::new(1)],
        },
        2,
    )));
    let scene = sources.scene();
    let area = Rect::new(0, 0, 80, 24);
    let layout = FrameLayout::from_scene(&scene, area);
    let text = rendered(80, 24, |frame| {
        frame.render_widget(&ScreenWidget::new(scene, &layout), frame.area());
    })
    .to_string();
    assert!(
        text.contains("Alpha") && !text.contains("Gamma"),
        "got {text:?}"
    );
}
