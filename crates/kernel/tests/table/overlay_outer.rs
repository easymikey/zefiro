use std::time::Duration;

use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        cue::Cue,
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        index::ViewIndex,
        model::Model,
        overlay::{MusicDirError, Overlay, OverlayName, SearchQuery, TextEntry},
        playlist::{PlaylistFileName, PlaylistFileNameError},
        setting_row::SettingRow,
        time::TimecodeError,
    },
    message::{
        BrowseRequest,
        HistoryRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        SearchEdit,
        SearchRequest,
        TextRequest,
    },
    update::{
        machine::Unhandled,
        overlay::{
            OverlayContentMessage,
            OverlayMessage,
            history::HistoryMessage,
            settings::SettingRowMessage,
        },
    },
};
use rstest::rstest;

use crate::support::{table::cell, track_at, update::send};

fn help() -> Overlay {
    Overlay::Help
}

fn search(input: &str, matches: Vec<usize>, selected_index: usize) -> Overlay {
    Overlay::Search(CursorOver {
        cursor: Cursor::at(matches.len(), selected_index),
        content: SearchQuery {
            input: input.to_string(),
            matches: matches.into_iter().map(ViewIndex::new).collect(),
        },
    })
}

fn entry<E>(input: &str, error: Option<E>) -> TextEntry<E> {
    TextEntry {
        input: input.to_string(),
        error,
    }
}

fn save(input: &str, error: Option<PlaylistFileNameError>) -> Overlay {
    Overlay::SavePlaylist(entry(input, error))
}

fn saved_name(input: &str) -> PlaylistFileName {
    PlaylistFileName::new(input).unwrap()
}

fn history(selected_index: usize, len: usize) -> Overlay {
    Overlay::History(CursorOver {
        cursor: Cursor::at(len, selected_index),
        content: (),
    })
}

fn settings(selected_index: usize) -> Overlay {
    Overlay::Settings(SettingRow::ALL[selected_index])
}

fn fresh_settings() -> Overlay {
    Overlay::Settings(SettingRow::Theme)
}

fn confirm_trash() -> Overlay {
    Overlay::ConfirmTrash(track_at("/music/sun.flac"))
}

fn track_details() -> Overlay {
    Overlay::TrackDetails(track_at("/tmp/a.flac"))
}

fn jump(input: &str, error: Option<TimecodeError>) -> Overlay {
    Overlay::JumpToTime(entry(input, error))
}

fn source_dir(input: &str, error: Option<MusicDirError>) -> Overlay {
    Overlay::MusicDir(entry(input, error))
}

fn open(overlay: Overlay) -> OverlayMessage {
    OverlayMessage::Open(overlay)
}

fn inner(overlay_content_message: OverlayContentMessage) -> OverlayMessage {
    OverlayMessage::Content(overlay_content_message)
}

fn text(message: TextRequest) -> OverlayMessage {
    inner(OverlayContentMessage::Text(message))
}

fn opened(cmd: Cmd) -> Cmd {
    Cmd::from(Cue::OverlayOpened).then(cmd)
}

fn closed(cmd: Cmd) -> Cmd {
    Cmd::from(Cue::OverlayClosed).then(cmd)
}

fn saved_music_dir(path: &str) -> Cmd {
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            music_dir: Some(std::path::PathBuf::from(path)),
            ..ConfigPatch::default()
        })),
        Effect::Animate(Cue::OverlayClosed),
    ])
}

fn holds() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::HoldForOverlay))
}

fn releases() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::Release))
}

#[rstest]
#[case::closed_opens_help(None, open(help()), Ok((Some(help()), opened(Cmd::none()))))]
#[case::help_reopen_keeps_help(Some(help()), open(help()), Ok((Some(help()), opened(Cmd::none()))))]
#[case::help_open_search_replaces_it(Some(help()), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(Cmd::none()))))]
#[case::search_reopen_resets_the_query(Some(search("mo", vec![0], 0)), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(Cmd::none()))))]
#[case::closed_opens_save(None, open(save("", None)), Ok((Some(save("", None)), opened(Cmd::none()))))]
#[case::save_reopen_resets_the_name(Some(save("mix", None)), open(save("", None)), Ok((Some(save("", None)), opened(Cmd::none()))))]
#[case::closed_opens_history(None, open(history(0, 0)), Ok((Some(history(0, 0)), opened(Cmd::none()))))]
#[case::history_reopen_resets_the_cursor(Some(history(2, 3)), open(history(0, 0)), Ok((Some(history(0, 0)), opened(Cmd::none()))))]
#[case::closed_opens_settings_and_holds(None, open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_reopen_resets_the_row_and_holds_again(Some(settings(3)), open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::help_open_settings_holds_like_any_other_open(Some(help()), open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_open_help_releases_what_it_held(Some(settings(3)), open(help()), Ok((Some(help()), opened(releases()))))]
#[case::settings_open_history_releases_what_it_held(Some(settings(3)), open(history(0, 0)), Ok((Some(history(0, 0)), opened(releases()))))]
#[case::closed_opens_confirm_trash(None, open(confirm_trash()), Ok((Some(confirm_trash()), opened(Cmd::none()))))]
#[case::closed_opens_track_details(None, open(track_details()), Ok((Some(track_details()), opened(Cmd::none()))))]
#[case::closed_opens_jump(None, open(jump("", None)), Ok((Some(jump("", None)), opened(Cmd::none()))))]
#[case::jump_reopen_resets_the_digits(Some(jump("5:", Some(TimecodeError::Malformed))), open(jump("", None)), Ok((Some(jump("", None)), opened(Cmd::none()))))]
#[case::closed_opens_source_dir_prefilled(None, open(source_dir("/music", None)), Ok((Some(source_dir("/music", None)), opened(Cmd::none()))))]
#[case::source_dir_reopen_resets_the_path(Some(source_dir("/x", Some(MusicDirError::Empty))), open(source_dir("/music", None)), Ok((Some(source_dir("/music", None)), opened(Cmd::none()))))]
#[case::closed_close_is_refused(None, OverlayMessage::Close, Err(Unhandled))]
#[case::help_closes(Some(help()), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::search_closes(Some(search("mo", vec![0], 0)), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::save_closes(Some(save("mix", None)), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::history_closes(Some(history(1, 3)), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::settings_closes_and_releases(Some(settings(3)), OverlayMessage::Close, Ok((None, closed(releases()))))]
#[case::confirm_trash_closes(Some(confirm_trash()), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::track_details_closes(Some(track_details()), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::jump_closes(Some(jump("5", None)), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::source_dir_closes(Some(source_dir("/x", None)), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::closed_confirm_is_refused(None, OverlayMessage::Confirm, Err(Unhandled))]
#[case::help_confirm_is_refused(Some(help()), OverlayMessage::Confirm, Err(Unhandled))]
#[case::track_details_confirm_is_refused(
    Some(track_details()),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::history_confirm_is_refused(
    Some(history(1, 3)),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::settings_confirm_is_refused(
    Some(settings(3)),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::search_confirm_plays_the_selected_match(Some(search("mo", vec![0, 2], 1)), OverlayMessage::Confirm, Ok((None, closed(Cmd::message(Message::Playback(PlaybackRequest::JumpTo(ViewIndex::new(2))))))))]
#[case::search_confirm_without_a_match_is_refused(Some(search("zzz", vec![], 0)), OverlayMessage::Confirm, Err(Unhandled))]
#[case::save_confirm_saves_under_the_name(Some(save("mix", None)), OverlayMessage::Confirm, Ok((None, closed(Cmd::message(Message::Browse(BrowseRequest::SavePlaylist(saved_name("mix"))))))))]
#[case::save_confirm_with_an_empty_name_stays_open_with_the_error(Some(save("", None)), OverlayMessage::Confirm, Ok((Some(save("", Some(PlaylistFileNameError::Empty))), Cmd::none())))]
#[case::confirm_trash_confirm_trashes_the_candidate(Some(confirm_trash()), OverlayMessage::Confirm, Ok((None, closed(Cmd::message(Message::Browse(BrowseRequest::Trash(kernel::domain::track::TrackSource::Local("/music/sun.flac".into()))))))))]
#[case::jump_confirm_seeks_to_the_parsed_time(Some(jump("1:40", None)), OverlayMessage::Confirm, Ok((None, closed(Cmd::message(Message::Playback(PlaybackRequest::SeekTo(Duration::from_secs(100))))))))]
#[case::jump_confirm_malformed_stays_open_with_the_error(Some(jump("5:", None)), OverlayMessage::Confirm, Ok((Some(jump("5:", Some(TimecodeError::Malformed))), Cmd::none())))]
#[case::jump_confirm_malformed_again_is_refused(
    Some(jump("5:", Some(TimecodeError::Malformed))),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::save_confirm_empty_again_is_refused(
    Some(save("", Some(PlaylistFileNameError::Empty))),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::source_dir_confirm_empty_again_is_refused(
    Some(source_dir("  ", Some(MusicDirError::Empty))),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::source_dir_confirm_saves_the_folder(Some(source_dir("/music", None)), OverlayMessage::Confirm, Ok((None, saved_music_dir("/music"))))]
#[case::music_dir_confirm_saves_the_folder_trimmed(Some(source_dir("  /music  ", None)), OverlayMessage::Confirm, Ok((None, saved_music_dir("/music"))))]
#[case::source_dir_confirm_empty_stays_open_with_the_error(Some(source_dir("  ", None)), OverlayMessage::Confirm, Ok((Some(source_dir("  ", Some(MusicDirError::Empty))), Cmd::none())))]
#[case::save_types_a_char(Some(save("mi", None)), text(TextRequest::Char('x')), Ok((Some(save("mix", None)), Cmd::none())))]
#[case::save_backspace_on_empty_is_refused(
    Some(save("", None)),
    text(TextRequest::Backspace),
    Err(Unhandled)
)]
#[case::jump_backspace_on_empty_is_refused(
    Some(jump("", None)),
    inner(OverlayContentMessage::Text(TextRequest::Backspace)),
    Err(Unhandled)
)]
#[case::save_backspace_erases(Some(save("mix", None)), text(TextRequest::Backspace), Ok((Some(save("mi", None)), Cmd::none())))]
#[case::save_types_a_char_and_clears_the_error(Some(save("", Some(PlaylistFileNameError::Empty))), text(TextRequest::Char('m')), Ok((Some(save("m", None)), Cmd::none())))]
#[case::source_dir_types_a_char_and_clears_the_error(Some(source_dir("", Some(MusicDirError::Empty))), text(TextRequest::Char('/')), Ok((Some(source_dir("/", None)), Cmd::none())))]
#[case::source_dir_backspace_clears_the_error(Some(source_dir("/x", Some(MusicDirError::Empty))), text(TextRequest::Backspace), Ok((Some(source_dir("/", None)), Cmd::none())))]
#[case::jump_types_a_digit_and_clears_the_error(Some(jump("5:", Some(TimecodeError::Malformed))), inner(OverlayContentMessage::Text(TextRequest::Char('3'))), Ok((Some(jump("5:3", None)), Cmd::none())))]
#[case::jump_refuses_a_letter(
    Some(jump("5", None)),
    inner(OverlayContentMessage::Text(TextRequest::Char('a'))),
    Err(Unhandled)
)]
#[case::search_types_a_char(
    Some(search("mo", vec![0], 0)),
    inner(OverlayContentMessage::Search(SearchRequest::Edit(SearchEdit::Char('o')))),
    Ok((Some(search("moo", vec![0], 0)), Cmd::none()))
)]
#[case::search_clear_on_an_empty_query_is_refused(
    Some(search("", vec![0, 1], 0)),
    inner(OverlayContentMessage::Search(SearchRequest::Edit(SearchEdit::Clear))),
    Err(Unhandled)
)]
#[case::search_enqueues_the_selected_match(Some(search("mo", vec![0, 2], 1)), inner(OverlayContentMessage::Search(SearchRequest::Enqueue)), Ok((Some(search("mo", vec![0, 2], 1)), Cmd::message(Message::Queue(QueueRequest::ToggleAt(ViewIndex::new(2)))))))]
#[case::search_enqueue_without_a_match_is_refused(
    Some(search("zzz", vec![], 0)),
    inner(OverlayContentMessage::Search(SearchRequest::Enqueue)),
    Err(Unhandled)
)]
#[case::history_navigates(Some(history(0, 3)), inner(OverlayContentMessage::History(HistoryMessage { request: HistoryRequest::Navigate(Direction::Next), rows: 3 })), Ok((Some(history(1, 3)), Cmd::none())))]
#[case::history_enqueues_the_entry_under_the_cursor(Some(history(1, 2)), inner(OverlayContentMessage::History(HistoryMessage { request: HistoryRequest::Enqueue, rows: 2 })), Ok((Some(history(1, 2)), Cmd::message(Message::Queue(QueueRequest::ToggleHistoryEntry(1))))))]
#[case::history_enqueue_past_the_end_is_refused(
    Some(history(0, 1)),
    inner(OverlayContentMessage::History(HistoryMessage { request: HistoryRequest::Enqueue, rows: 0 })),
    Err(Unhandled)
)]
#[case::closed_content_is_refused(None, text(TextRequest::Char('a')), Err(Unhandled))]
#[case::help_refuses_text(Some(help()), text(TextRequest::Char('a')), Err(Unhandled))]
#[case::search_refuses_history(Some(search("mo", vec![0], 0)), inner(OverlayContentMessage::History(HistoryMessage { request: HistoryRequest::SelectFirst, rows: 1 })), Err(Unhandled))]
#[case::save_refuses_search(
    Some(save("mix", None)),
    inner(OverlayContentMessage::Search(SearchRequest::Enqueue)),
    Err(Unhandled)
)]
#[case::history_refuses_search(
    Some(history(1, 3)),
    inner(OverlayContentMessage::Search(SearchRequest::Edit(SearchEdit::Char(
        'a'
    )))),
    Err(Unhandled)
)]
#[case::settings_refuses_text(
    Some(settings(3)),
    text(TextRequest::Backspace),
    Err(Unhandled)
)]
#[case::confirm_trash_refuses_settings(
    Some(confirm_trash()),
    inner(OverlayContentMessage::Settings(SettingRowMessage::Set(
        SettingRow::Theme
    ))),
    Err(Unhandled)
)]
#[case::track_details_refuses_text(
    Some(track_details()),
    inner(OverlayContentMessage::Text(TextRequest::Backspace)),
    Err(Unhandled)
)]
#[case::jump_refuses_search(
    Some(jump("5", None)),
    inner(OverlayContentMessage::Search(SearchRequest::Enqueue)),
    Err(Unhandled)
)]
#[case::source_dir_refuses_search(
    Some(source_dir("/x", None)),
    inner(OverlayContentMessage::Search(SearchRequest::Enqueue)),
    Err(Unhandled)
)]
fn overlay_cell(
    #[case] overlay: Option<Overlay>,
    #[case] overlay_message: OverlayMessage,
    #[case] expected: Result<
        (
            Option<Overlay>,
            <Option<Overlay> as kernel::update::machine::Machine>::Effect,
        ),
        Unhandled,
    >,
) {
    cell(overlay, overlay_message, expected);
}

fn opened_music_dir(music_dir: std::path::PathBuf) -> Option<Overlay> {
    let mut model = Model {
        music_dir,
        ..Model::default()
    };
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::MusicDir)),
    );
    model.workspace.overlay
}

#[test]
fn music_dir_prompt_is_prefilled_with_the_path() {
    let overlay = opened_music_dir("/music/mix tape".into());
    assert_eq!(overlay, Some(source_dir("/music/mix tape", None)));
}

#[cfg(unix)]
#[test]
fn music_dir_prompt_never_prefills_a_lossy_path() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let path = std::path::PathBuf::from(OsStr::from_bytes(b"/music/\xff"));
    let overlay = opened_music_dir(path);
    assert_eq!(overlay, Some(source_dir("", None)));
}
