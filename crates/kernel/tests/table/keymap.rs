use std::{path::Path, sync::Arc, time::Duration};

use insta::assert_snapshot;
use kernel::{
    cmd::Effect,
    domain::{
        chord::{Chord, ChordPrefix},
        cue::Cue,
        cursor_over::CursorOver,
        direction::Direction,
        geometry::Cells,
        key::{Key, KeyCode, KeyPress, Modifiers},
        keymap::{KeyContext, KeymapOverrides},
        overlay::{Overlay, OverlayName, TextEntry, TrashCandidate},
        setting_row::SettingRow,
        time::Moment,
        toast::Toast,
        track::Track,
        workspace::Workspace,
    },
    message::{
        BrowseRequest,
        HistoryRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        SearchEdit,
        SearchRequest,
        SettingRowRequest,
        TextRequest,
    },
    update::{keymap::lookup::route, machine::Unhandled, update},
};
use rstest::rstest;

use crate::support::keymap::{bindings, character};

const VISIBLE_ROWS: Cells = Cells(10);

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
    workspace.chord_prefix = Some(ChordPrefix::G);
    workspace
}

fn settings_on(row: SettingRow) -> Workspace {
    with_overlay(Overlay::Settings(row))
}

fn confirming_trash() -> Workspace {
    with_overlay(Overlay::ConfirmTrash(TrashCandidate {
        source: kernel::domain::track::TrackSource::Local("/music/moon.flac".into()),
        title: "Moon River".to_string(),
        artist: "Audrey Hepburn".to_string(),
    }))
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

fn confirm() -> Option<Message> {
    Some(Message::Overlay(OverlayRequest::Confirm))
}

#[rstest]
#[case::browse_question_mark_opens_help(
    browsing(),
    character('?'),
    Some(Message::Overlay(OverlayRequest::Open(OverlayName::Help)))
)]
#[case::browse_shift_left_seeks_far_back(browsing(), with(KeyCode::Left, Modifiers::SHIFT), Some(Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Previous, by: Duration::from_secs(30) })))]
#[case::browse_shift_right_seeks_far_forward(
    browsing(),
    with(KeyCode::Right, Modifiers::SHIFT),
    Some(Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Next, by: Duration::from_secs(30) }))
)]
#[case::browse_left_seeks_back(browsing(), plain(KeyCode::Left), Some(Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Previous, by: Duration::from_secs(5) })))]
#[case::browse_right_seeks_forward(
    browsing(),
    plain(KeyCode::Right),
    Some(Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Next, by: Duration::from_secs(5) }))
)]
#[case::browse_ctrl_u_pages_the_playlist(
    browsing(),
    Key::ctrl(KeyCode::Char('u')),
    Some(Message::Browse(BrowseRequest::PageBy(Direction::Previous)))
)]
#[case::help_swallows_a_hotkey(help(), character('n'), None)]
#[case::help_esc_closes(help(), plain(KeyCode::Esc), close())]
#[case::help_q_closes_instead_of_quitting(help(), character('q'), close())]
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
        Direction::Next
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
    history_request(HistoryRequest::Navigate(Direction::Next))
)]
#[case::history_down_navigates_down(
    history(),
    plain(KeyCode::Down),
    history_request(HistoryRequest::Navigate(Direction::Next))
)]
#[case::history_k_navigates_up(
    history(),
    character('k'),
    history_request(HistoryRequest::Navigate(Direction::Previous))
)]
#[case::history_capital_g_jumps_to_the_bottom(
    history(),
    character('G'),
    history_request(HistoryRequest::SelectLast)
)]
#[case::history_enter_enqueues(
    history(),
    plain(KeyCode::Enter),
    history_request(HistoryRequest::Enqueue)
)]
#[case::history_esc_closes(history(), plain(KeyCode::Esc), close())]
#[case::history_q_closes(history(), character('q'), close())]
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
#[case::history_swallows_a_hotkey(history(), character('n'), None)]
#[case::settings_j_navigates_down(
    settings_on(SettingRow::Theme),
    character('j'),
    settings_row(SettingRowRequest::Navigate(Direction::Next))
)]
#[case::settings_down_navigates_down(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Down),
    settings_row(SettingRowRequest::Navigate(Direction::Next))
)]
#[case::settings_k_navigates_up(
    settings_on(SettingRow::Theme),
    character('k'),
    settings_row(SettingRowRequest::Navigate(Direction::Previous))
)]
#[case::settings_up_navigates_up(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Up),
    settings_row(SettingRowRequest::Navigate(Direction::Previous))
)]
#[case::settings_l_steps_up(
    settings_on(SettingRow::Theme),
    character('l'),
    settings_row(SettingRowRequest::Step(Direction::Next))
)]
#[case::settings_right_steps_up(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Right),
    settings_row(SettingRowRequest::Step(Direction::Next))
)]
#[case::settings_h_steps_down(
    settings_on(SettingRow::Theme),
    character('h'),
    settings_row(SettingRowRequest::Step(Direction::Previous))
)]
#[case::settings_left_steps_down(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Left),
    settings_row(SettingRowRequest::Step(Direction::Previous))
)]
#[case::settings_space_activates_a_pick_row(
    settings_on(SettingRow::Theme),
    character(' '),
    settings_row(SettingRowRequest::Activate)
)]
#[case::settings_esc_closes(
    settings_on(SettingRow::Theme),
    plain(KeyCode::Esc),
    close()
)]
#[case::settings_q_closes(settings_on(SettingRow::Theme), character('q'), close())]
#[case::settings_enter_on_a_duration_row_still_activates(
    settings_on(SettingRow::Crossfade),
    plain(KeyCode::Enter),
    settings_row(SettingRowRequest::Activate)
)]
#[case::settings_space_on_a_duration_row_still_activates(
    settings_on(SettingRow::Crossfade),
    character(' '),
    settings_row(SettingRowRequest::Activate)
)]
#[case::settings_h_on_a_toggle_row_steps_never_seeks(
    settings_on(SettingRow::ReplayGain),
    character('h'),
    settings_row(SettingRowRequest::Step(Direction::Previous))
)]
#[case::settings_left_on_a_toggle_row_steps_never_seeks(
    settings_on(SettingRow::ReplayGain),
    plain(KeyCode::Left),
    settings_row(SettingRowRequest::Step(Direction::Previous))
)]
#[case::settings_l_on_a_toggle_row_steps_never_seeks(
    settings_on(SettingRow::ReplayGain),
    character('l'),
    settings_row(SettingRowRequest::Step(Direction::Next))
)]
#[case::settings_right_on_a_toggle_row_steps_never_seeks(
    settings_on(SettingRow::ReplayGain),
    plain(KeyCode::Right),
    settings_row(SettingRowRequest::Step(Direction::Next))
)]
#[case::settings_enter_on_a_toggle_row_activates_it(
    settings_on(SettingRow::ReplayGain),
    plain(KeyCode::Enter),
    settings_row(SettingRowRequest::Activate)
)]
#[case::settings_swallows_a_hotkey(
    settings_on(SettingRow::Theme),
    character('n'),
    None
)]
#[case::confirm_trash_y_accepts(confirming_trash(), character('y'), confirm())]
#[case::confirm_trash_enter_accepts(
    confirming_trash(),
    plain(KeyCode::Enter),
    confirm()
)]
#[case::confirm_trash_n_cancels(confirming_trash(), character('n'), close())]
#[case::confirm_trash_esc_cancels(confirming_trash(), plain(KeyCode::Esc), close())]
#[case::confirm_trash_q_closes(confirming_trash(), character('q'), close())]
#[case::confirm_trash_swallows_a_nav_key(confirming_trash(), character('j'), None)]
#[case::confirm_trash_swallows_its_own_hotkey(confirming_trash(), character('d'), None)]
#[case::confirm_trash_swallows_an_arrow(confirming_trash(), plain(KeyCode::Down), None)]
#[case::jump_types_a_digit(
    jumping(),
    character('4'),
    Some(Message::Overlay(OverlayRequest::Text(TextRequest::Char('4'))))
)]
#[case::jump_types_a_colon(
    jumping(),
    character(':'),
    Some(Message::Overlay(OverlayRequest::Text(TextRequest::Char(':'))))
)]
#[case::jump_backspace_erases(
    jumping(),
    plain(KeyCode::Backspace),
    Some(Message::Overlay(OverlayRequest::Text(TextRequest::Backspace)))
)]
#[case::jump_enter_confirms(jumping(), plain(KeyCode::Enter), confirm())]
#[case::jump_esc_cancels(jumping(), plain(KeyCode::Esc), close())]
#[case::jump_q_closes(jumping(), character('q'), close())]
#[case::jump_sends_a_letter_to_the_entry(
    jumping(),
    character('a'),
    typed_text(TextRequest::Char('a'))
)]
#[case::jump_sends_a_nav_key_to_the_entry(
    jumping(),
    character('j'),
    typed_text(TextRequest::Char('j'))
)]
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
#[case::an_overlay_key_with_no_binding_falls_through_to_typed_input(
    searching(),
    character('z'),
    search_edit(SearchEdit::Char('z'))
)]
#[case::a_base_key_with_no_playlist_binding_falls_back_to_global(
    browsing(),
    character('z'),
    Some(Message::Playback(PlaybackRequest::CycleSleep))
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
    model.workspace.overlay = overlay;
    model.workspace.toasts = vec![Toast::info("hello")];

    let press = KeyPress { key, typed };
    let before = model.clone();
    let routed = update(&mut model, Message::Key(press), Moment::default());
    assert_eq!(routed.is_err(), typed.code == KeyCode::Char('w'));

    match typed.code {
        KeyCode::Char('j') => {
            assert_eq!(model.workspace.browse.selected().get(), 1);
        }
        KeyCode::Char('a') => {
            assert!(matches!(
                &model.workspace.overlay,
                Some(Overlay::Search(cursor_over)) if cursor_over.content.input == "a"
            ));
        }
        KeyCode::Char('x') => {
            assert_eq!(model.workspace.chord_prefix, Some(ChordPrefix::G));
        }
        KeyCode::Char('w') => {
            assert!(matches!(routed, Err(Unhandled)));
            assert_eq!(model, before);
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
#[case::an_unbound_key(crate::support::model_with_tracks(3), 'w')]
#[case::a_refused_key(kernel::domain::model::Model::default(), 'j')]
fn cancelling_a_chord_handles_the_key(
    #[case] mut model: kernel::domain::model::Model,
    #[case] letter: char,
) {
    model.workspace.toasts = vec![Toast::info("hello")];
    let before = model.clone();
    model.workspace.chord_prefix = Some(ChordPrefix::G);
    let key = character(letter);

    let routed = update(
        &mut model,
        Message::Key(KeyPress { key, typed: key }),
        Moment::default(),
    );

    assert_eq!(routed, Ok(Vec::new()));
    assert_eq!(model.workspace.chord_prefix, None);
    assert_eq!(model.workspace.toasts, vec![Toast::info("hello")]);
    assert_eq!(model, before);
}

#[test]
fn q_in_help_closes_the_overlay_and_does_not_quit() {
    let mut model = kernel::domain::model::Model::default();
    model.workspace.overlay = Some(Overlay::Help);
    let key = character('q');

    let routed = update(
        &mut model,
        Message::Key(KeyPress { key, typed: key }),
        Moment::default(),
    );

    assert_eq!(model.workspace.overlay, None);
    assert_eq!(routed, Ok(vec![Effect::Animate(Cue::OverlayClosed)]));
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
            Chord::Sequence { prefix, key } => {
                workspace.chord_prefix = Some(prefix);
                key
            }
        };
        let press = KeyPress { key, typed: key };
        assert_eq!(route(&workspace, press), Some(binding.message), "{chord}");
    }
}
