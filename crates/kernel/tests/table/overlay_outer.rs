use std::time::Duration;

use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect, LibraryCmd, MacosCmd, ScanMode},
    domain::{
        cue::Cue,
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        index::ViewIndex,
        io_error::IoError,
        model::Model,
        overlay::{
            Field,
            MusicDirError,
            Overlay,
            OverlayName,
            SearchQuery,
            ServerPrompt,
            TextEntry,
            Verdict,
        },
        playlist::{PlaylistFileName, PlaylistFileNameError},
        revision::Revision,
        server::{
            Account,
            Connection,
            Credential,
            Endpoint,
            EndpointError,
            Secret,
            SecretError,
            ServerName,
            ServerStatus,
            UserName,
            UserNameError,
        },
        setting_row::SettingRow,
        time::{Moment, TimecodeError},
    },
    message::{
        BrowseRequest,
        ConfigEvent,
        HistoryRequest,
        LibraryEvent,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        SearchRequest,
        ServerRequest,
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
    music_dir(input, error, None)
}

fn music_dir(
    input: &str,
    error: Option<MusicDirError>,
    verdict: Option<Verdict>,
) -> Overlay {
    Overlay::MusicDir {
        text_entry: entry(input, error),
        verdict,
        revision: None,
        folders: CursorOver::default(),
    }
}
fn readable_dir(input: &str) -> Overlay {
    music_dir(input, None, Some(Verdict::Readable))
}

fn link_step(
    input: &str,
    error: Option<EndpointError>,
    reached_field: Field,
) -> Overlay {
    Overlay::AddServer(ServerPrompt {
        link_text_entry: entry(input, error),
        reached_field,
        ..ServerPrompt::default()
    })
}

const LINK: &str = "https://music.example.com";

fn endpoint() -> Endpoint {
    Endpoint::parse("https://music.example.com").unwrap()
}

fn user_name() -> UserName {
    UserName::new("alice").unwrap()
}

fn user_step(input: &str, error: Option<UserNameError>) -> Overlay {
    Overlay::AddServer(ServerPrompt {
        link_text_entry: entry(LINK, None),
        user_text_entry: entry(input, error),
        field: Field::User,
        reached_field: Field::User,
        ..ServerPrompt::default()
    })
}

fn password_prompt(input: &str, error: Option<SecretError>) -> ServerPrompt {
    ServerPrompt {
        link_text_entry: entry(LINK, None),
        user_text_entry: entry("alice", None),
        password_text_entry: entry(input, error),
        field: Field::Password,
        reached_field: Field::Password,
        ..ServerPrompt::default()
    }
}

fn password_step(input: &str, error: Option<SecretError>) -> Overlay {
    Overlay::AddServer(password_prompt(input, error))
}

fn connecting(password: &str) -> Overlay {
    Overlay::AddServer(ServerPrompt {
        origin_server_name: Some(ServerName::new("music.example.com")),
        server_status: Some(ServerStatus::Connecting),
        ..password_prompt(password, None)
    })
}

fn added_server(password: &str) -> Cmd {
    Cmd::message(Message::Server(ServerRequest::Add {
        connection: Connection {
            account: Account {
                server_name: ServerName::new("music.example.com"),
                endpoint: endpoint(),
                user_name: user_name(),
            },
            credential: Credential::Typed(Secret::new(password).unwrap()),
        },
        origin_server_name: None,
    }))
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
    .then(Cmd::message(Message::Config(
        ConfigEvent::MusicDirReloaded(std::path::PathBuf::from(path)),
    )))
}

fn holds() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::HoldForOverlay))
}

fn releases() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::Release))
}

#[rstest]
#[case::closed_opens_help(None, open(help()), Ok((Some(help()), opened(Cmd::none()))))]
#[case::help_open_search_replaces_it(Some(help()), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(Cmd::none()))))]
#[case::search_reopen_resets_the_query(Some(search("mo", vec![0], 0)), open(search("", vec![0, 1], 0)), Ok((Some(search("", vec![0, 1], 0)), opened(Cmd::none()))))]
#[case::closed_opens_settings_and_holds(None, open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_reopen_resets_the_row_and_holds_again(Some(settings(3)), open(fresh_settings()), Ok((Some(fresh_settings()), opened(holds()))))]
#[case::settings_open_help_releases_what_it_held(Some(settings(3)), open(help()), Ok((Some(help()), opened(releases()))))]
#[case::closed_close_is_refused(None, OverlayMessage::Close, Err(Unhandled))]
#[case::help_closes(Some(help()), OverlayMessage::Close, Ok((None, closed(Cmd::none()))))]
#[case::settings_closes_and_releases(Some(settings(3)), OverlayMessage::Close, Ok((None, closed(releases()))))]
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
#[case::search_confirm_plays_the_selected_match(Some(search("mo", vec![0, 2], 1)), OverlayMessage::Confirm, Ok((None, closed(Cmd::message(Message::Browse(BrowseRequest::JumpTo(ViewIndex::new(2))))))))]
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
#[case::source_dir_confirm_saves_the_folder(Some(readable_dir("/music")), OverlayMessage::Confirm, Ok((None, saved_music_dir("/music"))))]
#[case::music_dir_confirm_saves_the_folder_trimmed(Some(readable_dir("  /music  ")), OverlayMessage::Confirm, Ok((None, saved_music_dir("/music"))))]
#[case::music_dir_confirm_drops_a_trailing_slash(Some(readable_dir("/music/")), OverlayMessage::Confirm, Ok((None, saved_music_dir("/music"))))]
#[case::music_dir_confirm_keeps_the_root(Some(readable_dir("/")), OverlayMessage::Confirm, Ok((None, saved_music_dir("/"))))]
#[case::music_dir_confirm_before_an_answer_shows_the_check_is_running(
    Some(source_dir("/music", None)),
    OverlayMessage::Confirm,
    Ok((Some(source_dir("/music", Some(MusicDirError::Pending))), Cmd::none()))
)]
#[case::music_dir_confirm_again_while_checking_is_refused(
    Some(source_dir("/music", Some(MusicDirError::Pending))),
    OverlayMessage::Confirm,
    Err(Unhandled)
)]
#[case::music_dir_confirm_of_an_unreadable_path_checks_it_again(
    Some(music_dir("/loop", None, Some(Verdict::Unreadable(IoError::Other)))),
    OverlayMessage::Confirm,
    Ok((Some(music_dir("/loop", Some(MusicDirError::Pending), Some(Verdict::Unreadable(IoError::Other)))), Cmd::none()))
)]
#[case::music_dir_confirm_of_a_missing_path_checks_it_again(
    Some(music_dir("/gone", None, Some(Verdict::Missing))),
    OverlayMessage::Confirm,
    Ok((Some(music_dir("/gone", Some(MusicDirError::Pending), Some(Verdict::Missing))), Cmd::none()))
)]
#[case::music_dir_confirm_of_a_file_checks_it_again(
    Some(music_dir("/a.flac", None, Some(Verdict::NotADirectory))),
    OverlayMessage::Confirm,
    Ok((Some(music_dir("/a.flac", Some(MusicDirError::Pending), Some(Verdict::NotADirectory))), Cmd::none()))
)]
#[case::music_dir_confirm_of_a_denied_folder_checks_it_again(Some(music_dir("/Desktop", None, Some(Verdict::Denied))), OverlayMessage::Confirm, Ok((Some(music_dir("/Desktop", Some(MusicDirError::Pending), Some(Verdict::Denied))), Cmd::none())))]
#[case::source_dir_confirm_empty_stays_open_with_the_error(Some(source_dir("  ", None)), OverlayMessage::Confirm, Ok((Some(source_dir("  ", Some(MusicDirError::Empty))), Cmd::none())))]
#[case::add_server_link_moves_to_the_user(Some(link_step(LINK, None, Field::Link)), OverlayMessage::Confirm, Ok((Some(user_step("", None)), Cmd::none())))]
#[case::add_server_bad_link_moves_on_with_the_scheme_error(Some(link_step("music.example.com", None, Field::Link)), OverlayMessage::Confirm, Ok((Some(Overlay::AddServer(ServerPrompt { link_text_entry: entry("music.example.com", Some(EndpointError::Scheme)), field: Field::User, reached_field: Field::User, ..ServerPrompt::default() })), Cmd::none())))]
#[case::add_server_empty_user_moves_on_with_its_error(Some(user_step("", None)), OverlayMessage::Confirm, Ok((Some(Overlay::AddServer(ServerPrompt { user_text_entry: entry("", Some(UserNameError::Empty)), ..password_prompt("", None) })), Cmd::none())))]
#[case::add_server_user_moves_to_the_password(Some(user_step("alice", None)), OverlayMessage::Confirm, Ok((Some(password_step("", None)), Cmd::none())))]
#[case::add_server_empty_password_stays(Some(password_step("", None)), OverlayMessage::Confirm, Ok((Some(password_step("", Some(SecretError::Empty))), Cmd::none())))]
#[case::add_server_password_adds_the_server_and_stays_connecting(Some(password_step("hunter 2", None)), OverlayMessage::Confirm, Ok((Some(connecting("hunter 2")), added_server("hunter 2"))))]
#[case::add_server_link_refuses_a_space(
    Some(link_step("https://", None, Field::Link)),
    text(TextRequest::Char(' ')),
    Err(Unhandled)
)]
#[case::add_server_user_refuses_a_space(
    Some(user_step("al", None)),
    text(TextRequest::Char(' ')),
    Err(Unhandled)
)]
#[case::add_server_link_types_a_char_and_keeps_the_live_scheme_verdict(Some(link_step("x", Some(EndpointError::Scheme), Field::User)), text(TextRequest::Char('/')), Ok((Some(link_step("x/", Some(EndpointError::Scheme), Field::User)), Cmd::none())))]
#[case::add_server_link_after_the_scheme_asks_for_the_host(Some(link_step("https:/", Some(EndpointError::Scheme), Field::User)), text(TextRequest::Char('/')), Ok((Some(link_step("https://", Some(EndpointError::Host), Field::User)), Cmd::none())))]
#[case::add_server_link_typed_to_a_valid_link_clears_the_verdict(Some(link_step("https://", Some(EndpointError::Host), Field::User)), text(TextRequest::Char('m')), Ok((Some(link_step("https://m", None, Field::User)), Cmd::none())))]
#[case::add_server_link_with_a_port_out_of_range_shows_the_host_verdict(Some(link_step("https://music.example.com:6553", Some(EndpointError::Host), Field::User)), text(TextRequest::Char('6')), Ok((Some(link_step("https://music.example.com:65536", Some(EndpointError::Host), Field::User)), Cmd::none())))]
#[case::add_server_link_with_a_user_shows_the_user_info_verdict(Some(link_step("https://alice", Some(EndpointError::Host), Field::User)), text(TextRequest::Char('@')), Ok((Some(link_step("https://alice@", Some(EndpointError::UserInfo), Field::User)), Cmd::none())))]
#[case::add_server_link_with_a_query_shows_the_query_verdict(Some(link_step("https://music.example.com", Some(EndpointError::Host), Field::User)), text(TextRequest::Char('?')), Ok((Some(link_step("https://music.example.com?", Some(EndpointError::Query), Field::User)), Cmd::none())))]
#[case::add_server_untouched_link_backspace_shows_no_verdict(Some(link_step("https://m", None, Field::Link)), text(TextRequest::Backspace), Ok((Some(link_step("https://", None, Field::Link)), Cmd::none())))]
#[case::add_server_untouched_link_cleared_shows_no_verdict(Some(link_step("https://m", None, Field::Link)), text(TextRequest::Clear), Ok((Some(link_step("", None, Field::Link)), Cmd::none())))]
#[case::add_server_password_takes_a_space(Some(password_step("a", None)), text(TextRequest::Char(' ')), Ok((Some(password_step("a ", None)), Cmd::none())))]
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
    inner(OverlayContentMessage::Search(SearchRequest::Edit(TextRequest::Char('o')))),
    Ok((Some(search("moo", vec![0], 0)), Cmd::none()))
)]
#[case::search_clear_on_an_empty_query_is_refused(
    Some(search("", vec![0, 1], 0)),
    inner(OverlayContentMessage::Search(SearchRequest::Edit(TextRequest::Clear))),
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
    inner(OverlayContentMessage::Search(SearchRequest::Edit(TextRequest::Char(
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

fn text_entry_of(overlay: Option<&Overlay>) -> Option<&TextEntry<MusicDirError>> {
    if let Some(Overlay::MusicDir { text_entry, .. }) = overlay {
        Some(text_entry)
    } else {
        None
    }
}

#[test]
fn music_dir_prompt_is_prefilled_with_the_path() {
    let overlay = opened_music_dir("/music/mix tape".into());
    assert_eq!(
        text_entry_of(overlay.as_ref()),
        Some(&entry("/music/mix tape", None))
    );
}

fn probes(cmd: &Cmd) -> Vec<(std::path::PathBuf, Revision)> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::Library(LibraryCmd::Probe { path, revision }) = effect {
                Some((path.clone(), *revision))
            } else {
                None
            }
        })
        .collect()
}

fn verdict_of(model: &Model) -> Option<Verdict> {
    if let Some(Overlay::MusicDir { verdict, .. }) = &model.workspace.overlay {
        *verdict
    } else {
        None
    }
}

fn probed_prompt(music_dir: &str) -> (Model, Revision) {
    let mut model = Model {
        music_dir: music_dir.into(),
        ..Model::default()
    };
    let cmd = crate::support::update::update(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::MusicDir)),
        Moment::default(),
    )
    .unwrap();
    let probed = probes(&cmd);
    assert_eq!(probed.len(), 1, "opening the prompt probes its path once");
    let (_, revision) = probed[0].clone();
    (model, revision)
}

fn answered(
    model: &mut Model,
    verdict: Verdict,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    crate::support::update::update(
        model,
        Message::Library(LibraryEvent::Checked { verdict, revision }),
        Moment::default(),
    )
}

#[test]
fn opening_the_music_dir_prompt_probes_the_path_without_its_trailing_slash() {
    let mut model = Model {
        music_dir: "/music/".into(),
        ..Model::default()
    };
    let cmd = crate::support::update::update(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::MusicDir)),
        Moment::default(),
    )
    .unwrap();
    let paths: Vec<_> = probes(&cmd).into_iter().map(|(path, _)| path).collect();
    assert_eq!(paths, vec![std::path::PathBuf::from("/music")]);
}

#[test]
fn typing_keeps_the_shown_verdict_and_enter_checks_the_new_path_before_applying_it() {
    let (mut model, revision) = probed_prompt("/music");
    assert_eq!(
        answered(&mut model, Verdict::Readable, revision),
        Ok(Cmd::none())
    );
    let cmd = crate::support::update::update(
        &mut model,
        Message::Overlay(OverlayRequest::Text(TextRequest::Char('x'))),
        Moment::default(),
    )
    .unwrap();
    let probed = probes(&cmd);
    assert_eq!(probed.len(), 1);
    assert_eq!(probed[0].0, std::path::PathBuf::from("/musicx"));
    assert_ne!(probed[0].1, revision);
    assert_eq!(verdict_of(&model), Some(Verdict::Readable));
    send(&mut model, Message::Overlay(OverlayRequest::Confirm));
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/musicx", Some(MusicDirError::Pending)))
    );
    assert!(answered(&mut model, Verdict::Readable, probed[0].1).is_ok());
    assert_eq!(model.workspace.overlay, None);
    assert_eq!(model.music_dir, std::path::PathBuf::from("/musicx"));
}

#[rstest]
#[case::readable(Verdict::Readable)]
#[case::missing(Verdict::Missing)]
#[case::not_a_folder(Verdict::NotADirectory)]
#[case::denied(Verdict::Denied)]
#[case::unreadable(Verdict::Unreadable(IoError::Other))]
fn a_fresh_verdict_is_shown(#[case] verdict: Verdict) {
    let (mut model, revision) = probed_prompt("/music");
    assert_eq!(answered(&mut model, verdict, revision), Ok(Cmd::none()));
    assert_eq!(verdict_of(&model), Some(verdict));
}

#[test]
fn a_stale_verdict_is_dropped() {
    let (mut model, stale) = probed_prompt("/music");
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Text(TextRequest::Char('x'))),
    );
    assert_eq!(
        answered(&mut model, Verdict::Readable, stale),
        Err(Unhandled)
    );
    assert_eq!(verdict_of(&model), None);
}

#[test]
fn a_verdict_without_the_prompt_is_refused() {
    let mut model = Model::default();
    assert_eq!(
        answered(&mut model, Verdict::Readable, Revision::default()),
        Err(Unhandled)
    );
}

#[test]
fn enter_applies_the_music_dir_only_after_a_readable_answer() {
    let (mut model, revision) = probed_prompt("/new");
    assert_eq!(
        crate::support::update::update(
            &mut model,
            Message::Overlay(OverlayRequest::Confirm),
            Moment::default(),
        ),
        Ok(Cmd::none())
    );
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/new", Some(MusicDirError::Pending)))
    );
    assert!(answered(&mut model, Verdict::Readable, revision).is_ok());
    assert_eq!(model.workspace.overlay, None);
    assert_eq!(model.music_dir, std::path::PathBuf::from("/new"));
}

fn rechecked(model: &mut Model) -> Revision {
    let cmd = crate::support::update::update(
        model,
        Message::Overlay(OverlayRequest::Confirm),
        Moment::default(),
    )
    .unwrap();
    let probed = probes(&cmd);
    assert_eq!(
        probed.len(),
        1,
        "Enter on a settled verdict probes the path once"
    );
    assert_eq!(verdict_of(model), Some(Verdict::Denied));
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/new", Some(MusicDirError::Pending)))
    );
    let (path, revision) = probed[0].clone();
    assert_eq!(path, std::path::PathBuf::from("/new"));
    revision
}

#[test]
fn enter_after_a_denied_answer_checks_again_and_a_readable_answer_applies_the_folder() {
    let (mut model, revision) = probed_prompt("/new");
    assert_eq!(
        answered(&mut model, Verdict::Denied, revision),
        Ok(Cmd::none())
    );
    let fresh = rechecked(&mut model);
    assert_ne!(fresh, revision);
    assert!(answered(&mut model, Verdict::Readable, fresh).is_ok());
    assert_eq!(model.workspace.overlay, None);
    assert_eq!(model.music_dir, std::path::PathBuf::from("/new"));
}

#[test]
fn enter_after_a_denied_answer_and_denied_again_opens_the_privacy_pane_once() {
    let (mut model, revision) = probed_prompt("/new");
    assert_eq!(
        answered(&mut model, Verdict::Denied, revision),
        Ok(Cmd::none())
    );
    let fresh = rechecked(&mut model);
    assert_eq!(
        answered(&mut model, Verdict::Denied, fresh),
        Ok(Cmd::from(Effect::Macos(MacosCmd::Privacy)))
    );
    assert_eq!(verdict_of(&model), Some(Verdict::Denied));
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/new", None))
    );
    assert_eq!(answered(&mut model, Verdict::Denied, fresh), Err(Unhandled));
}

#[rstest]
#[case::missing(Verdict::Missing)]
#[case::not_a_folder(Verdict::NotADirectory)]
#[case::unreadable(Verdict::Unreadable(IoError::Other))]
fn enter_after_a_denied_answer_shows_the_reason_of_another_answer(
    #[case] verdict: Verdict,
) {
    let (mut model, revision) = probed_prompt("/new");
    assert_eq!(
        answered(&mut model, Verdict::Denied, revision),
        Ok(Cmd::none())
    );
    let fresh = rechecked(&mut model);
    assert_eq!(answered(&mut model, verdict, fresh), Ok(Cmd::none()));
    assert_eq!(verdict_of(&model), Some(verdict));
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/new", None))
    );
}

#[test]
fn enter_after_a_denied_answer_drops_the_stale_answer() {
    let (mut model, revision) = probed_prompt("/new");
    assert_eq!(
        answered(&mut model, Verdict::Denied, revision),
        Ok(Cmd::none())
    );
    assert_ne!(rechecked(&mut model), revision);
    assert_eq!(
        answered(&mut model, Verdict::Readable, revision),
        Err(Unhandled)
    );
    assert_eq!(verdict_of(&model), Some(Verdict::Denied));
    assert_eq!(
        text_entry_of(model.workspace.overlay.as_ref()),
        Some(&entry("/new", Some(MusicDirError::Pending)))
    );
}

#[cfg(unix)]
#[test]
fn music_dir_prompt_never_prefills_a_lossy_path() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let path = std::path::PathBuf::from(OsStr::from_bytes(b"/music/\xff"));
    let overlay = opened_music_dir(path);
    assert_eq!(text_entry_of(overlay.as_ref()), Some(&entry("", None)));
}

fn confirmed_music_dir(input: &str) -> (Model, Cmd) {
    let mut model = Model {
        music_dir: "/music".into(),
        ..Model::default()
    };
    model.workspace.overlay = Some(readable_dir(input));
    let cmd = crate::support::update::update(
        &mut model,
        Message::Overlay(OverlayRequest::Confirm),
        Moment::default(),
    )
    .unwrap();
    (model, cmd)
}

fn fresh_scans(cmd: &Cmd) -> Vec<std::path::PathBuf> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::Library(LibraryCmd::Scan {
                music_dir,
                revision: _,
                mode: ScanMode::Fresh,
            }) = effect
            {
                Some(music_dir.clone())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_confirmed_new_music_dir_applies_at_once() {
    let (model, cmd) = confirmed_music_dir("/new");

    assert_eq!(model.music_dir, std::path::PathBuf::from("/new"));
    assert!(cmd.effects().any(|effect| *effect
        == Effect::Config(ConfigCmd::Save(ConfigPatch {
            music_dir: Some("/new".into()),
            ..ConfigPatch::default()
        }))));
    assert_eq!(fresh_scans(&cmd), vec![std::path::PathBuf::from("/new")]);
}

#[test]
fn a_confirmed_current_music_dir_rescans_nothing() {
    let (model, cmd) = confirmed_music_dir("/music");

    assert_eq!(model.music_dir, std::path::PathBuf::from("/music"));
    assert_eq!(fresh_scans(&cmd), Vec::<std::path::PathBuf>::new());
}
