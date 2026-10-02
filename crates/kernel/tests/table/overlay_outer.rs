use std::{sync::Arc, time::Duration};

use kernel::{
    BrowseRequest,
    Cmd,
    ConfigCmd,
    ConfigPatch,
    Cue,
    Direction,
    Effect,
    Overlay,
    PlaybackRequest,
    PlaylistRequest,
    QueueRequest,
    SearchEdit,
    TextRequest,
    domain::{
        Cursor,
        CursorOver,
        DeleteCandidate,
        JumpDigits,
        MusicDirError,
        SearchQuery,
        SettingRow,
        TextEntry,
        TimecodeError,
        Track,
        ViewIndex,
        playlist::{PlaylistFileName, PlaylistNameError},
    },
    update::overlay::{
        FollowUp,
        HistoryError,
        HistoryMessage,
        HistoryPick,
        InnerMessage,
        JumpError,
        OverlayError,
        OverlayMessage,
        OverlayOutcome,
        SearchError,
        SearchMessage,
        SettingsCursorMessage,
    },
};
use rstest::rstest;

use crate::support::{table::cell, titled_track, track_at};

fn help() -> Overlay {
    Overlay::Help
}

fn search(input: &str, matches: Vec<usize>, selected: usize) -> Overlay {
    Overlay::Search(CursorOver {
        cursor: Cursor::with_len(matches.len()).at(selected),
        content: SearchQuery {
            input: input.to_string(),
            matches,
        },
    })
}

fn titled(titles: &[&str]) -> Vec<Arc<Track>> {
    titles
        .iter()
        .enumerate()
        .map(|(index, title)| titled_track(&format!("/tmp/{index}.flac"), title, ""))
        .collect()
}

fn entry(input: &str) -> TextEntry {
    TextEntry {
        input: input.to_string(),
    }
}

fn save(input: &str, error: Option<PlaylistNameError>) -> Overlay {
    Overlay::SavePlaylist {
        typed: entry(input),
        error,
    }
}

fn saved_name(input: &str) -> PlaylistFileName {
    PlaylistFileName::new(input).unwrap()
}

fn history(selected: usize, len: usize) -> Overlay {
    Overlay::History(CursorOver {
        cursor: Cursor::with_len(len).at(selected),
        content: (),
    })
}

fn settings(selected: usize) -> Overlay {
    let rows = SettingRow::all(&[]);
    Overlay::Settings {
        selected: rows[selected],
    }
}

fn fresh_settings() -> Overlay {
    Overlay::Settings {
        selected: SettingRow::Theme,
    }
}

fn candidate() -> DeleteCandidate {
    DeleteCandidate {
        index: ViewIndex::new(1),
        title: "Sun Song".to_string(),
        artist: "Someone".to_string(),
    }
}

fn confirm_delete() -> Overlay {
    Overlay::ConfirmDelete(candidate())
}

fn track_details() -> Overlay {
    Overlay::TrackDetails(track_at("/tmp/a.flac"))
}

fn jump(input: &str, error: Option<TimecodeError>) -> Overlay {
    Overlay::JumpToTime(JumpDigits {
        input: input.to_string(),
        error,
    })
}

fn source_dir(input: &str, error: Option<MusicDirError>) -> Overlay {
    Overlay::MusicDir {
        typed: entry(input),
        error,
    }
}

fn open(overlay: Overlay) -> OverlayMessage {
    OverlayMessage::Open(overlay)
}

fn inner(message: InnerMessage) -> OverlayMessage {
    OverlayMessage::Inner(message)
}

fn text(message: TextRequest) -> OverlayMessage {
    inner(InnerMessage::Text(message))
}

fn opened(follow_up: Option<FollowUp>) -> OverlayOutcome {
    OverlayOutcome {
        cmd: Cue::OverlayOpened.into(),
        follow_up,
    }
}

fn closed(follow_up: Option<FollowUp>) -> OverlayOutcome {
    OverlayOutcome {
        cmd: Cue::OverlayClosed.into(),
        follow_up,
    }
}

fn saved_music_dir(path: &str) -> Cmd {
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .music_dir(std::path::PathBuf::from(path))
                .build(),
        )),
        Effect::Animate(Cue::OverlayClosed),
    ])
}

fn holds() -> Option<FollowUp> {
    Some(FollowUp::Playback(PlaybackRequest::HoldForOverlay))
}

fn releases() -> Option<FollowUp> {
    Some(FollowUp::Playback(PlaybackRequest::Release))
}

type Cell = crate::support::table::Cell<Option<Overlay>>;

#[rstest]
#[case::closed_opens_help(None, open(help()), Ok((Some(help()), opened(None))))]
#[case::help_reopen_keeps_help(Some(help()), open(help()), Ok((Some(help()), opened(None))))]
#[case::help_open_search_replaces_it(Some(help()), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(None))))]
#[case::search_reopen_resets_the_query(Some(search("mo", vec![0], 0)), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(None))))]
#[case::closed_opens_save(None, open(save("", None)), Ok((Some(save("", None)), opened(None))))]
#[case::save_reopen_resets_the_name(Some(save("mix", None)), open(save("", None)), Ok((Some(save("", None)), opened(None))))]
#[case::closed_opens_history(None, open(history(0, 0)), Ok((Some(history(0, 0)), opened(None))))]
#[case::history_reopen_resets_the_cursor(Some(history(2, 3)), open(history(0, 0)), Ok((Some(history(0, 0)), opened(None))))]
#[case::closed_opens_settings_and_holds(None, open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_reopen_resets_the_row_and_holds_again(Some(settings(3)), open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::help_open_settings_holds_like_any_other_open(Some(help()), open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_open_help_releases_what_it_held(Some(settings(3)), open(help()), Ok((Some(help()), opened(releases()))))]
#[case::settings_open_history_releases_what_it_held(Some(settings(3)), open(history(0, 0)), Ok((Some(history(0, 0)), opened(releases()))))]
#[case::closed_opens_confirm_delete(None, open(confirm_delete()), Ok((Some(confirm_delete()), opened(None))))]
#[case::closed_opens_track_details(None, open(track_details()), Ok((Some(track_details()), opened(None))))]
#[case::closed_opens_jump(None, open(jump("", None)), Ok((Some(jump("", None)), opened(None))))]
#[case::jump_reopen_resets_the_digits(Some(jump("5:", Some(TimecodeError::Malformed))), open(jump("", None)), Ok((Some(jump("", None)), opened(None))))]
#[case::closed_opens_source_dir_prefilled(None, open(source_dir("/music", None)), Ok((Some(source_dir("/music", None)), opened(None))))]
#[case::source_dir_reopen_resets_the_path(Some(source_dir("/x", Some(MusicDirError::Empty))), open(source_dir("/music", None)), Ok((Some(source_dir("/music", None)), opened(None))))]
#[case::closed_close_is_refused(
    None,
    OverlayMessage::Close,
    Err(OverlayError::WhileClosed)
)]
#[case::help_closes(Some(help()), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::search_closes(Some(search("mo", vec![0], 0)), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::save_closes(Some(save("mix", None)), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::history_closes(Some(history(1, 3)), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::settings_closes_and_releases(Some(settings(3)), OverlayMessage::Close, Ok((None, closed(releases()))))]
#[case::confirm_delete_closes(Some(confirm_delete()), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::track_details_closes(Some(track_details()), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::jump_closes(Some(jump("5", None)), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::source_dir_closes(Some(source_dir("/x", None)), OverlayMessage::Close, Ok((None, closed(None))))]
#[case::closed_confirm_is_refused(
    None,
    OverlayMessage::Confirm,
    Err(OverlayError::WhileClosed)
)]
#[case::help_confirm_is_refused(
    Some(help()),
    OverlayMessage::Confirm,
    Err(OverlayError::NoConfirm)
)]
#[case::track_details_confirm_is_refused(
    Some(track_details()),
    OverlayMessage::Confirm,
    Err(OverlayError::NoConfirm)
)]
#[case::history_confirm_is_refused(
    Some(history(1, 3)),
    OverlayMessage::Confirm,
    Err(OverlayError::NoConfirm)
)]
#[case::settings_confirm_closes_and_releases(Some(settings(3)), OverlayMessage::Confirm, Ok((None, closed(releases()))))]
#[case::search_confirm_plays_the_selected_match(Some(search("mo", vec![0, 2], 1)), OverlayMessage::Confirm, Ok((None, closed(Some(FollowUp::Playlist(PlaylistRequest::JumpTo(ViewIndex::new(2))))))))]
#[case::search_confirm_without_a_match_is_refused(Some(search("zzz", vec![], 0)), OverlayMessage::Confirm, Err(OverlayError::NothingSelected))]
#[case::save_confirm_saves_under_the_name(Some(save("mix", None)), OverlayMessage::Confirm, Ok((None, closed(Some(FollowUp::Browse(BrowseRequest::SavePlaylist(saved_name("mix"))))))))]
#[case::save_confirm_with_an_empty_name_stays_open_with_the_error(Some(save("", None)), OverlayMessage::Confirm, Ok((Some(save("", Some(PlaylistNameError::Empty))), OverlayOutcome::default())))]
#[case::confirm_delete_confirm_trashes_the_candidate(Some(confirm_delete()), OverlayMessage::Confirm, Ok((None, closed(Some(FollowUp::Browse(BrowseRequest::Trash(ViewIndex::new(1))))))))]
#[case::jump_confirm_seeks_to_the_parsed_time(Some(jump("1:40", None)), OverlayMessage::Confirm, Ok((None, closed(Some(FollowUp::Playback(PlaybackRequest::SeekTo(Duration::from_secs(100))))))))]
#[case::jump_confirm_malformed_stays_open_with_the_error(Some(jump("5:", None)), OverlayMessage::Confirm, Ok((Some(jump("5:", Some(TimecodeError::Malformed))), OverlayOutcome::default())))]
#[case::source_dir_confirm_saves_the_folder(Some(source_dir("/music", None)), OverlayMessage::Confirm, Ok((None, OverlayOutcome::from(saved_music_dir("/music")))))]
#[case::source_dir_confirm_empty_stays_open_with_the_error(Some(source_dir("  ", None)), OverlayMessage::Confirm, Ok((Some(source_dir("  ", Some(MusicDirError::Empty))), OverlayOutcome::default())))]
#[case::save_types_a_char(Some(save("mi", None)), text(TextRequest::Char('x')), Ok((Some(save("mix", None)), OverlayOutcome::default())))]
#[case::save_backspace_erases(Some(save("mix", None)), text(TextRequest::Backspace), Ok((Some(save("mi", None)), OverlayOutcome::default())))]
#[case::save_types_a_char_and_clears_the_error(Some(save("", Some(PlaylistNameError::Empty))), text(TextRequest::Char('m')), Ok((Some(save("m", None)), OverlayOutcome::default())))]
#[case::source_dir_types_a_char_and_clears_the_error(Some(source_dir("", Some(MusicDirError::Empty))), text(TextRequest::Char('/')), Ok((Some(source_dir("/", None)), OverlayOutcome::default())))]
#[case::source_dir_backspace_clears_the_error(Some(source_dir("/x", Some(MusicDirError::Empty))), text(TextRequest::Backspace), Ok((Some(source_dir("/", None)), OverlayOutcome::default())))]
#[case::jump_types_a_digit_and_clears_the_error(Some(jump("5:", Some(TimecodeError::Malformed))), inner(InnerMessage::Jump(TextRequest::Char('3'))), Ok((Some(jump("5:3", None)), OverlayOutcome::default())))]
#[case::jump_refuses_a_letter(
    Some(jump("5", None)),
    inner(InnerMessage::Jump(TextRequest::Char('a'))),
    Err(OverlayError::Jump(JumpError::NotTimecodeChar))
)]
#[case::search_types_a_char(
    Some(search("mo", vec![0], 0)),
    inner(InnerMessage::Search(SearchMessage::Edit(SearchEdit::Char('o'), titled(&["moo", "zzz"])))),
    Ok((Some(search("moo", vec![0], 0)), OverlayOutcome::default()))
)]
#[case::search_editing_installs_fresh_matches(
    Some(search("m", vec![], 0)),
    inner(InnerMessage::Search(SearchMessage::Edit(SearchEdit::Char('o'), titled(&["mo", "zzz", "moon"])))),
    Ok((Some(search("mo", vec![0, 2], 0)), OverlayOutcome::default()))
)]
#[case::search_enqueues_the_selected_match(Some(search("mo", vec![0, 2], 1)), inner(InnerMessage::Search(SearchMessage::Enqueue)), Ok((Some(search("mo", vec![0, 2], 1)), OverlayOutcome::from(FollowUp::Queue(QueueRequest::EnqueueTrack(ViewIndex::new(2)))))))]
#[case::search_enqueue_without_a_match_is_refused(
    Some(search("zzz", vec![], 0)),
    inner(InnerMessage::Search(SearchMessage::Enqueue)),
    Err(OverlayError::Search(SearchError::NothingSelected))
)]
#[case::history_navigates(Some(history(0, 3)), inner(InnerMessage::History(HistoryMessage::Navigate { direction: Direction::Next, len: 3 })), Ok((Some(history(1, 3)), OverlayOutcome::default())))]
#[case::history_enqueues_the_resolved_entry(Some(history(1, 2)), inner(InnerMessage::History(HistoryMessage::Enqueue(HistoryPick::Queued(ViewIndex::new(3))))), Ok((Some(history(1, 2)), OverlayOutcome::from(FollowUp::Queue(QueueRequest::EnqueueTrack(ViewIndex::new(3)))))))]
#[case::history_enqueue_of_a_missing_entry_is_refused(
    Some(history(0, 1)),
    inner(InnerMessage::History(HistoryMessage::Enqueue(HistoryPick::Missing))),
    Err(OverlayError::History(HistoryError::NotInLibrary))
)]
#[case::closed_inner_is_refused(
    None,
    text(TextRequest::Char('a')),
    Err(OverlayError::WhileClosed)
)]
#[case::help_refuses_text(
    Some(help()),
    text(TextRequest::Char('a')),
    Err(OverlayError::WrongOverlay)
)]
#[case::search_refuses_history(Some(search("mo", vec![0], 0)), inner(InnerMessage::History(HistoryMessage::Top)), Err(OverlayError::WrongOverlay))]
#[case::save_refuses_jump(
    Some(save("mix", None)),
    inner(InnerMessage::Jump(TextRequest::Char('1'))),
    Err(OverlayError::WrongOverlay)
)]
#[case::history_refuses_search(
    Some(history(1, 3)),
    inner(InnerMessage::Search(SearchMessage::Edit(SearchEdit::Char('a'), vec![]))),
    Err(OverlayError::WrongOverlay)
)]
#[case::settings_refuses_text(
    Some(settings(3)),
    text(TextRequest::Backspace),
    Err(OverlayError::WrongOverlay)
)]
#[case::confirm_delete_refuses_settings(
    Some(confirm_delete()),
    inner(InnerMessage::Settings(SettingsCursorMessage::Navigate(
        SettingRow::Theme
    ))),
    Err(OverlayError::WrongOverlay)
)]
#[case::track_details_refuses_jump(
    Some(track_details()),
    inner(InnerMessage::Jump(TextRequest::Backspace)),
    Err(OverlayError::WrongOverlay)
)]
#[case::jump_refuses_text(
    Some(jump("5", None)),
    text(TextRequest::Char('1')),
    Err(OverlayError::WrongOverlay)
)]
#[case::source_dir_refuses_search(
    Some(source_dir("/x", None)),
    inner(InnerMessage::Search(SearchMessage::Enqueue)),
    Err(OverlayError::WrongOverlay)
)]
fn overlay_cell(
    #[case] start: Option<Overlay>,
    #[case] message: OverlayMessage,
    #[case] expected: Cell,
) {
    cell(start, message, expected);
}
