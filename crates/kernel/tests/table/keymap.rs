use std::{path::Path, sync::Arc};

use insta::assert_snapshot;
use kernel::{
    BrowseRequest,
    HistoryRequest,
    JumpRequest,
    Key,
    KeyCode,
    KeyPress,
    Message,
    Modifiers,
    Moment,
    Nudge,
    OverlayName,
    OverlayRequest,
    PlaybackRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    TextRequest,
    Toast,
    domain::{
        Chord,
        ChordPrefix,
        CursorOver,
        DeleteCandidate,
        JumpDigits,
        KeyContext,
        KeymapOverrides,
        Overlay,
        PlaylistIndex,
        SeekStep,
        SettingRow,
        SettingsCursor,
        TextEntry,
        Track,
        Workspace,
    },
    update::{
        keymap::{Bindings, KeyOutcome, route},
        update,
    },
};
use rstest::rstest;

use crate::support::keymap::{bindings, character};

const VISIBLE_ROWS: usize = 10;

fn browsing() -> Workspace {
    Workspace::default()
}

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
    workspace.chord = Some(ChordPrefix::G);
    workspace
}

fn settings_on(row: SettingRow) -> Workspace {
    with_overlay(Overlay::Settings(SettingsCursor { selected: row }))
}

fn confirming_delete() -> Workspace {
    with_overlay(Overlay::ConfirmDelete(DeleteCandidate {
        track: PlaylistIndex::new(0),
        title: "Moon River".to_string(),
        artist: "Audrey Hepburn".to_string(),
    }))
}

fn jumping() -> Workspace {
    with_overlay(Overlay::JumpToTime(JumpDigits::default()))
}

fn showing_track_details() -> Workspace {
    with_overlay(Overlay::TrackDetails(Arc::new(Track::listed(Path::new(
        "/tmp/track.flac",
    )))))
}

fn saving_a_playlist() -> Workspace {
    with_overlay(Overlay::SavePlaylist {
        typed: TextEntry::default(),
        error: None,
    })
}

fn naming_a_source_dir() -> Workspace {
    with_overlay(Overlay::SourceDir {
        typed: TextEntry::default(),
        error: None,
    })
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

fn settings_row(message: SettingsRowRequest) -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Settings(message)))
}

fn close() -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Close))
}

fn confirm() -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Confirm))
}

#[rstest]
#[case::browse_question_mark_opens_help(
    browsing(),
    character('?'),
    Some(Message::Overlay(OverlayRequest::Open(OverlayName::Help)))
)]
#[case::browse_shift_left_seeks_far_back(browsing(), with(KeyCode::Left, Modifiers::SHIFT), Some(Message::Playback(PlaybackRequest::SeekBy(SeekStep::new(-30)))))]
#[case::browse_shift_right_seeks_far_forward(
    browsing(),
    with(KeyCode::Right, Modifiers::SHIFT),
    Some(Message::Playback(PlaybackRequest::SeekBy(SeekStep::new(30))))
)]
#[case::browse_left_seeks_back(browsing(), plain(KeyCode::Left), Some(Message::Playback(PlaybackRequest::SeekBy(SeekStep::new(-5)))))]
#[case::browse_right_seeks_forward(
    browsing(),
    plain(KeyCode::Right),
    Some(Message::Playback(PlaybackRequest::SeekBy(SeekStep::new(5))))
)]
#[case::browse_ctrl_u_pages_the_playlist(
    browsing(),
    Key::ctrl(KeyCode::Char('u')),
    Some(Message::Browse(BrowseRequest::PageBy(Nudge::Up)))
)]
#[case::help_swallows_a_hotkey(help(), character('n'), None)]
#[case::help_esc_closes(help(), plain(KeyCode::Esc), close())]
#[case::search_types_a_letter_that_is_a_hotkey(
    searching(),
    character('a'),
    search_edit(SearchEdit::Char('a'))
)]
#[case::search_types_j(searching(), character('j'), search_edit(SearchEdit::Char('j')))]
#[case::search_types_k(searching(), character('k'), search_edit(SearchEdit::Char('k')))]
#[case::search_types_a_digit_that_is_a_hotkey(
    searching(),
    character('7'),
    search_edit(SearchEdit::Char('7'))
)]
#[case::search_ctrl_w_deletes_a_word(
    searching(),
    Key::ctrl(KeyCode::Char('w')),
    search_edit(SearchEdit::DeleteWord)
)]
#[case::search_ctrl_u_clears_the_query(
    searching(),
    Key::ctrl(KeyCode::Char('u')),
    search_edit(SearchEdit::Clear)
)]
#[case::search_alt_backspace_deletes_a_word(
    searching(),
    with(KeyCode::Backspace, Modifiers::ALT),
    search_edit(SearchEdit::DeleteWord)
)]
#[case::search_super_backspace_clears_the_query(
    searching(),
    with(KeyCode::Backspace, Modifiers::SUPER),
    search_edit(SearchEdit::Clear)
)]
#[case::search_down_navigates(
    searching(),
    plain(KeyCode::Down),
    Some(Message::Overlay(OverlayRequest::Search(SearchRequest::Navigate(
        Nudge::Down
    ))))
)]
#[case::search_tab_enqueues(
    searching(),
    plain(KeyCode::Tab),
    Some(Message::Overlay(OverlayRequest::Search(SearchRequest::Enqueue)))
)]
#[case::history_j_navigates_down(
    history(),
    character('j'),
    history_request(HistoryRequest::Navigate(Nudge::Down))
)]
#[case::history_down_navigates_down(
    history(),
    plain(KeyCode::Down),
    history_request(HistoryRequest::Navigate(Nudge::Down))
)]
#[case::history_k_navigates_up(
    history(),
    character('k'),
    history_request(HistoryRequest::Navigate(Nudge::Up))
)]
#[case::history_capital_g_jumps_to_the_bottom(
    history(),
    character('G'),
    history_request(HistoryRequest::Bottom)
)]
#[case::history_enter_enqueues(
    history(),
    plain(KeyCode::Enter),
    history_request(HistoryRequest::Enqueue)
)]
#[case::history_esc_closes(history(), plain(KeyCode::Esc), close())]
#[case::history_g_arms_the_chord(
    history(),
    character('g'),
    Some(Message::Browse(BrowseRequest::ChordPrefix(ChordPrefix::G)))
)]
#[case::history_gg_jumps_to_the_top(
    history_after_g(),
    character('g'),
    history_request(HistoryRequest::Top)
)]
#[case::history_swallows_a_hotkey(history(), character('n'), None)]
#[case::settings_j_navigates_down(
    settings_on(SettingRow::Theme),
    character('j'),
    settings_row(SettingsRowRequest::Navigate(Nudge::Down))
)]
#[case::settings_down_navigates_down(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Down),
    settings_row(SettingsRowRequest::Navigate(Nudge::Down))
)]
#[case::settings_k_navigates_up(
    settings_on(SettingRow::Theme),
    character('k'),
    settings_row(SettingsRowRequest::Navigate(Nudge::Up))
)]
#[case::settings_up_navigates_up(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Up),
    settings_row(SettingsRowRequest::Navigate(Nudge::Up))
)]
#[case::settings_l_adjusts_up(
    settings_on(SettingRow::Theme),
    character('l'),
    settings_row(SettingsRowRequest::Adjust(Nudge::Up))
)]
#[case::settings_right_adjusts_up(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Right),
    settings_row(SettingsRowRequest::Adjust(Nudge::Up))
)]
#[case::settings_h_adjusts_down(
    settings_on(SettingRow::Theme),
    character('h'),
    settings_row(SettingsRowRequest::Adjust(Nudge::Down))
)]
#[case::settings_left_adjusts_down(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Left),
    settings_row(SettingsRowRequest::Adjust(Nudge::Down))
)]
#[case::settings_space_activates_a_pick_row(
    settings_on(SettingRow::Theme),
    character(' '),
    settings_row(SettingsRowRequest::Activate)
)]
#[case::settings_esc_closes(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Esc),
    close()
)]
#[case::settings_enter_on_a_duration_row_still_activates(
    settings_on(SettingRow::Crossfade),
    plain(KeyCode::Enter),
    settings_row(SettingsRowRequest::Activate)
)]
#[case::settings_space_on_a_duration_row_still_activates(
    settings_on(SettingRow::Crossfade),
    character(' '),
    settings_row(SettingsRowRequest::Activate)
)]
#[case::settings_h_on_a_toggle_row_adjusts_never_seeks(
    settings_on(SettingRow::Replaygain),
    character('h'),
    settings_row(SettingsRowRequest::Adjust(Nudge::Down))
)]
#[case::settings_left_on_a_toggle_row_adjusts_never_seeks(
    settings_on(SettingRow::Replaygain),
    plain(KeyCode::Left),
    settings_row(SettingsRowRequest::Adjust(Nudge::Down))
)]
#[case::settings_l_on_a_toggle_row_adjusts_never_seeks(
    settings_on(SettingRow::Replaygain),
    character('l'),
    settings_row(SettingsRowRequest::Adjust(Nudge::Up))
)]
#[case::settings_right_on_a_toggle_row_adjusts_never_seeks(
    settings_on(SettingRow::Replaygain),
    plain(KeyCode::Right),
    settings_row(SettingsRowRequest::Adjust(Nudge::Up))
)]
#[case::settings_enter_on_a_toggle_row_activates_it(
    settings_on(SettingRow::Replaygain),
    plain(KeyCode::Enter),
    settings_row(SettingsRowRequest::Activate)
)]
#[case::settings_swallows_a_hotkey(
    settings_on(SettingRow::Theme),
    character('n'),
    None
)]
#[case::confirm_delete_y_accepts(confirming_delete(), character('y'), confirm())]
#[case::confirm_delete_enter_accepts(
    confirming_delete(),
    plain(KeyCode::Enter),
    confirm()
)]
#[case::confirm_delete_n_cancels(confirming_delete(), character('n'), close())]
#[case::confirm_delete_esc_cancels(confirming_delete(), plain(KeyCode::Esc), close())]
#[case::confirm_delete_swallows_a_nav_key(confirming_delete(), character('j'), None)]
#[case::confirm_delete_swallows_its_own_hotkey(
    confirming_delete(),
    character('d'),
    None
)]
#[case::confirm_delete_swallows_an_arrow(
    confirming_delete(),
    plain(KeyCode::Down),
    None
)]
#[case::jump_types_a_digit(
    jumping(),
    character('4'),
    Some(Message::Overlay(OverlayRequest::Jump(JumpRequest::Char('4'))))
)]
#[case::jump_types_a_colon(
    jumping(),
    character(':'),
    Some(Message::Overlay(OverlayRequest::Jump(JumpRequest::Char(':'))))
)]
#[case::jump_backspace_erases(
    jumping(),
    plain(KeyCode::Backspace),
    Some(Message::Overlay(OverlayRequest::Jump(JumpRequest::Backspace)))
)]
#[case::jump_enter_confirms(jumping(), plain(KeyCode::Enter), confirm())]
#[case::jump_esc_cancels(jumping(), plain(KeyCode::Esc), close())]
#[case::jump_swallows_a_letter(jumping(), character('a'), None)]
#[case::jump_swallows_a_nav_key(jumping(), character('j'), None)]
#[case::jump_swallows_an_arrow(jumping(), plain(KeyCode::Down), None)]
#[case::track_details_esc_closes(showing_track_details(), plain(KeyCode::Esc), close())]
#[case::track_details_its_own_hotkey_closes(
    showing_track_details(),
    character('i'),
    close()
)]
#[case::track_details_enter_closes(
    showing_track_details(),
    plain(KeyCode::Enter),
    close()
)]
#[case::track_details_quit_closes(showing_track_details(), character('q'), close())]
#[case::track_details_delete_closes(showing_track_details(), character('d'), close())]
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
#[case::a_save_prompt_erases(
    saving_a_playlist(),
    plain(KeyCode::Backspace),
    typed_text(TextRequest::Backspace)
)]
#[case::a_save_prompt_enter_confirms(
    saving_a_playlist(),
    plain(KeyCode::Enter),
    confirm()
)]
#[case::a_save_prompt_esc_cancels(saving_a_playlist(), plain(KeyCode::Esc), close())]
#[case::a_save_prompt_swallows_an_arrow(
    saving_a_playlist(),
    plain(KeyCode::Down),
    None
)]
#[case::a_source_dir_prompt_types_a_letter(
    naming_a_source_dir(),
    character('j'),
    typed_text(TextRequest::Char('j'))
)]
#[case::a_source_dir_prompt_esc_cancels(
    naming_a_source_dir(),
    plain(KeyCode::Esc),
    close()
)]
fn routed_key(
    #[case] mut workspace: Workspace,
    #[case] key: Key,
    #[case] expected: Option<Message>,
) {
    workspace.bindings = Bindings::new(&KeymapOverrides::default());
    workspace.visible_rows = VISIBLE_ROWS;
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&workspace, press), expected);
}

#[rstest]
#[case::space_toggles(character(' '), character(' '), None)]
#[case::j_moves_down(character('j'), character('j'), None)]
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
#[case::unbound_key_changes_nothing(character('w'), character('w'), None)]
fn a_key_press_routes_through_update(
    #[case] key: Key,
    #[case] typed: Key,
    #[case] overlay: Option<Overlay>,
) {
    let mut model = crate::support::model_with_tracks(3);
    model.workspace.bindings = Bindings::new(&KeymapOverrides::default());
    model.workspace.overlay = overlay;
    model.workspace.toast = Some(Toast::info("hello".to_string()));

    let press = KeyPress { key, typed };
    let _ = update(&mut model, Message::Key(press), Moment::default()).unwrap();

    match typed.code {
        KeyCode::Char('j') => {
            assert_eq!(model.workspace.browse.selected().get(), 1);
        }
        KeyCode::Char('a') => {
            assert!(matches!(
                &model.workspace.overlay,
                Some(Overlay::Search(cursor_over)) if cursor_over.rows.input == "a"
            ));
        }
        KeyCode::Char('x') => {
            assert_eq!(model.workspace.chord, Some(ChordPrefix::G));
        }
        KeyCode::Char('w') => {
            assert!(model.workspace.toast.is_some());
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
            assert!(model.workspace.toast.is_none());
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
    workspace.bindings = Bindings::new(&KeymapOverrides::default());
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
                binding.outcome
            )
        })
        .collect();
    assert_snapshot!(rendered.join("\n"));
}

fn fixed_message(outcome: &KeyOutcome) -> Option<Message> {
    match outcome {
        KeyOutcome::Message(message) => Some(message.clone()),
        KeyOutcome::TypeChar(_) => None,
    }
}

#[test]
fn every_compiled_binding_is_what_its_chord_routes_to() {
    let config = KeymapOverrides::default();
    let table = Bindings::new(&config);
    let browsable = bindings(&config)
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
            Chord::Sequence { prefix, key } => {
                workspace.chord = Some(prefix);
                key
            }
        };
        workspace.bindings = table.clone();
        let press = KeyPress { key, typed: key };
        assert_eq!(
            route(&workspace, press),
            fixed_message(&binding.outcome),
            "{chord}"
        );
    }
}
