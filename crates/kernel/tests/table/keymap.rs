use std::{path::Path, sync::Arc};

use insta::assert_snapshot;
use kernel::{
    domain::{
        chord::{Chord, ChordPrefix},
        cursor_over::CursorOver,
        direction::Direction,
        geometry::Cells,
        key::{Key, KeyCode, KeyPress, Modifiers},
        keymap::{KeyContext, KeymapOverrides},
        overlay::{Overlay, TextEntry},
        setting_row::SettingRow,
        time::Moment,
        toast::Toast,
        track::Track,
        workspace::Workspace,
    },
    message::{
        HistoryRequest,
        Message,
        OverlayRequest,
        SearchEdit,
        SearchRequest,
        SettingRowRequest,
        TextRequest,
    },
    update::{keymap::lookup::route, update},
};
use rstest::rstest;

use crate::support::keymap::{bindings, character};

const VISIBLE_ROWS: Cells = Cells(10);

fn with_overlay(overlay: Overlay) -> Workspace {
    let mut workspace = Workspace::default();
    workspace.overlay = Some(overlay);
    workspace
}

fn help() -> Workspace {
    with_overlay(Overlay::Help)
}

fn searching() -> Workspace {
    with_overlay(Overlay::Search(CursorOver::default()))
}

fn history() -> Workspace {
    with_overlay(Overlay::History(CursorOver::default()))
}

fn history_after_g() -> Workspace {
    let mut workspace = history();
    workspace.chord_prefix = Some(ChordPrefix::G);
    workspace
}

fn settings_on(row: SettingRow) -> Workspace {
    with_overlay(Overlay::Settings(row))
}

fn confirming_trash() -> Workspace {
    with_overlay(Overlay::ConfirmTrash(Arc::new(Track::listed(Path::new(
        "/music/moon.flac",
    )))))
}

fn jumping() -> Workspace {
    with_overlay(Overlay::JumpToTime(TextEntry::default()))
}

fn showing_track_details() -> Workspace {
    with_overlay(Overlay::TrackDetails(Arc::new(Track::listed(Path::new(
        "/tmp/track.flac",
    )))))
}

fn saving_a_playlist() -> Workspace {
    with_overlay(Overlay::SavePlaylist(TextEntry::default()))
}

fn naming_a_source_dir() -> Workspace {
    with_overlay(Overlay::MusicDir(TextEntry::default()))
}

fn typed_text(message: TextRequest) -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Text(message)))
}

fn plain(code: KeyCode) -> Key {
    Key::plain(code)
}

fn with(code: KeyCode, modifiers: Modifiers) -> Key {
    Key { code, modifiers }
}

fn search_edit(edit: SearchEdit) -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Search(
        SearchRequest::Edit(edit),
    )))
}

fn history_request(request: HistoryRequest) -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::History(request)))
}

fn settings_row(setting_row_request: SettingRowRequest) -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Settings(
        setting_row_request,
    )))
}

fn close() -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Close))
}

#[rstest]
#[case::help_swallows_a_hotkey(help(), character('n'), None)]
#[case::help_q_closes_instead_of_quitting(help(), character('q'), close())]
#[case::search_alt_backspace_deletes_a_word(
    searching(),
    with(KeyCode::Backspace, Modifiers::ALT),
    search_edit(SearchEdit::DeleteWord)
)]
#[case::history_g_arms_the_chord(
    history(),
    character('g'),
    Some(Message::ChordPrefix(ChordPrefix::G))
)]
#[case::history_gg_jumps_to_the_top(
    history_after_g(),
    character('g'),
    history_request(HistoryRequest::SelectFirst)
)]
#[case::settings_left_on_a_toggle_row_steps_never_seeks(
    settings_on(SettingRow::ReplayGain),
    plain(KeyCode::Left),
    settings_row(SettingRowRequest::Step(Direction::Previous))
)]
#[case::confirm_trash_swallows_its_own_hotkey(confirming_trash(), character('d'), None)]
#[case::jump_types_a_digit(
    jumping(),
    character('4'),
    Some(Message::Overlay(OverlayRequest::Text(TextRequest::Char('4'))))
)]
#[case::jump_swallows_an_arrow(jumping(), plain(KeyCode::Down), None)]
#[case::track_details_an_arrow_closes(
    showing_track_details(),
    plain(KeyCode::Down),
    close()
)]
#[case::a_save_prompt_types_a_letter(
    saving_a_playlist(),
    character('q'),
    typed_text(TextRequest::Char('q'))
)]
#[case::a_source_dir_prompt_types_a_letter(
    naming_a_source_dir(),
    character('j'),
    typed_text(TextRequest::Char('j'))
)]
fn routed_key(
    #[case] mut workspace: Workspace,
    #[case] key: Key,
    #[case] expected: Option<Message>,
) {
    workspace.visible_rows = VISIBLE_ROWS;
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&workspace, press), expected);
}

#[rstest]
#[case::typing_overlay_uses_typed(
    character('ф'),
    character('a'),
    Some(Overlay::Search(CursorOver::default()))
)]
#[case::chording_overlay_uses_key(
    character('g'),
    character('x'),
    Some(Overlay::History(CursorOver::default()))
)]
fn a_key_press_routes_through_update(
    #[case] key: Key,
    #[case] typed: Key,
    #[case] overlay: Option<Overlay>,
) {
    let mut model = crate::support::model_with_tracks(3);
    model.workspace.overlay = overlay;
    model.workspace.toasts = vec![Toast::info("hello")];

    let press = KeyPress { key, typed };
    let routed = update(&mut model, Message::Key(press), Moment::default());
    assert!(routed.is_ok());

    match typed.code {
        KeyCode::Char('a') => {
            assert!(matches!(
                &model.workspace.overlay,
                Some(Overlay::Search(cursor_over)) if cursor_over.content.input == "a"
            ));
        }
        KeyCode::Char('x') => {
            assert_eq!(model.workspace.chord_prefix, Some(ChordPrefix::G));
        }
        KeyCode::Char(_)
        | KeyCode::Enter
        | KeyCode::Esc
        | KeyCode::Backspace
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Tab
        | KeyCode::PageUp
        | KeyCode::PageDown => {
            assert!(model.workspace.toasts.is_empty());
        }
    }
}

#[rstest]
#[case::digits('0', '9')]
#[case::lowercase_letters('a', 'z')]
fn the_search_query_swallows_every_printable_hotkey(
    #[case] from: char,
    #[case] to: char,
) {
    let mut workspace = searching();
    workspace.visible_rows = VISIBLE_ROWS;
    for typed in from..=to {
        let key = character(typed);
        let press = KeyPress { key, typed: key };
        assert_eq!(
            route(&workspace, press),
            search_edit(SearchEdit::Char(typed)),
            "'{typed}' must type into the query, not fire a hotkey"
        );
    }
}

#[test]
fn the_default_keymap_compiles_to_this_table() {
    let rendered: Vec<String> = bindings(&KeymapOverrides::default())
        .iter()
        .map(|binding| {
            format!(
                "{:<14} {:<16} {:?}",
                format!("{:?}", binding.key_context),
                binding.pattern.to_string(),
                binding.message
            )
        })
        .collect();
    assert_snapshot!(rendered.join("\n"));
}

#[test]
fn every_compiled_binding_is_what_its_chord_routes_to() {
    let keymap_overrides = KeymapOverrides::default();
    let browsable = bindings(&keymap_overrides)
        .into_iter()
        .filter(|binding| {
            matches!(
                binding.key_context,
                KeyContext::Global | KeyContext::Playlist
            )
        })
        .filter_map(|binding| {
            let chord = binding.pattern.chord()?;
            Some((binding, chord))
        });
    for (binding, chord) in browsable {
        let mut workspace = Workspace::default();
        let key = match chord {
            Chord::Key(key) => key,
            Chord::Sequence { chord_prefix, key } => {
                workspace.chord_prefix = Some(chord_prefix);
                key
            }
        };
        let press = KeyPress { key, typed: key };
        assert_eq!(route(&workspace, press), Some(binding.message), "{chord}");
    }
}
