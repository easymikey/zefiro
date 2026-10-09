use crate::{
    domain::{
        chord::{Chord, ChordPrefix, KeyPattern},
        direction::Direction,
        key::{Key, KeyCode, Modifiers},
        keymap::{Action, KeyContext},
        overlay::OverlayName,
    },
    message::{
        HistoryRequest,
        Message,
        OverlayRequest,
        SearchRequest,
        SettingRowRequest,
        TextRequest,
    },
    update::keymap::chord::{KeyBinding, bare, key, letter, plain, row},
};

fn held(modifiers: Modifiers, code: KeyCode) -> KeyPattern {
    KeyPattern::Chord(Chord::Key(Key::new(code, modifiers)))
}

fn overlay(request: OverlayRequest) -> Message {
    Message::Overlay(request)
}

fn search(request: SearchRequest) -> Message {
    overlay(OverlayRequest::Search(request))
}

fn edit(text_request: TextRequest) -> Message {
    search(SearchRequest::Edit(text_request))
}

fn text(text_request: TextRequest) -> Message {
    overlay(OverlayRequest::Text(text_request))
}

fn history(request: HistoryRequest) -> Message {
    overlay(OverlayRequest::History(request))
}

fn close() -> Message {
    overlay(OverlayRequest::Close)
}

fn confirm() -> Message {
    overlay(OverlayRequest::Confirm)
}

fn rows_in(
    key_context: KeyContext,
    rows: Vec<(KeyPattern, Message)>,
) -> Vec<KeyBinding> {
    rows.into_iter()
        .map(|(pattern, message)| KeyBinding {
            pattern,
            message,
            action: None,
            key_context,
        })
        .collect()
}

fn edits(request: fn(TextRequest) -> Message) -> Vec<(KeyPattern, Message)> {
    vec![
        (
            held(Modifiers::CTRL, KeyCode::Char('u')),
            request(TextRequest::Clear),
        ),
        (
            held(Modifiers::CTRL, KeyCode::Char('w')),
            request(TextRequest::DeleteWord),
        ),
        (
            held(Modifiers::ALT, KeyCode::Backspace),
            request(TextRequest::DeleteWord),
        ),
        (
            held(Modifiers::SUPER, KeyCode::Backspace),
            request(TextRequest::Clear),
        ),
        (plain(KeyCode::Backspace), request(TextRequest::Backspace)),
    ]
}

fn text_prompt_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::TextPrompt,
        [
            vec![
                (plain(KeyCode::Enter), confirm()),
                (plain(KeyCode::Esc), close()),
            ],
            edits(text),
        ]
        .concat(),
    )
}

fn search_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::Search,
        [
            edits(edit),
            vec![
                (plain(KeyCode::Esc), close()),
                (plain(KeyCode::Enter), confirm()),
                (
                    plain(KeyCode::Down),
                    search(SearchRequest::Navigate(Direction::Next)),
                ),
                (
                    plain(KeyCode::Up),
                    search(SearchRequest::Navigate(Direction::Previous)),
                ),
                (plain(KeyCode::Tab), search(SearchRequest::Enqueue)),
            ],
        ]
        .concat(),
    )
}

fn help_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::Help,
        vec![
            (plain(KeyCode::Esc), close()),
            (letter('q'), close()),
            (held(Modifiers::CTRL, KeyCode::Char('k')), close()),
        ],
    )
}

fn history_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::History,
        vec![
            (
                KeyPattern::Chord(Chord::Sequence {
                    chord_prefix: ChordPrefix::G,
                    key: ChordPrefix::G.key(),
                }),
                history(HistoryRequest::SelectFirst),
            ),
            (plain(KeyCode::Esc), close()),
            (letter('q'), close()),
            (plain(KeyCode::Enter), history(HistoryRequest::Enqueue)),
            (
                letter('j'),
                history(HistoryRequest::Navigate(Direction::Next)),
            ),
            (
                plain(KeyCode::Down),
                history(HistoryRequest::Navigate(Direction::Next)),
            ),
            (
                letter('k'),
                history(HistoryRequest::Navigate(Direction::Previous)),
            ),
            (
                plain(KeyCode::Up),
                history(HistoryRequest::Navigate(Direction::Previous)),
            ),
            (letter('G'), history(HistoryRequest::SelectLast)),
        ],
    )
}

#[rustfmt::skip]
fn settings_rows() -> Vec<KeyBinding> {
    use SettingRowRequest::{Activate, Navigate, Step};

    use crate::domain::keymap::Action::{SettingsActivate, SettingsClose, SettingsNavigateDown, SettingsNavigateUp, SettingsStepDown, SettingsStepUp};
    let row = row(KeyContext::Settings);
    let settings = |request| Message::Overlay(OverlayRequest::Settings(request));
    vec![
        row(SettingsClose, bare(KeyCode::Esc), close()),
        row(SettingsClose, key('q'), close()),
        row(SettingsActivate, bare(KeyCode::Enter), settings(Activate)),
        row(SettingsActivate, key(' '), settings(Activate)),
        row(SettingsNavigateDown, key('j'), settings(Navigate(Direction::Next))),
        row(SettingsNavigateDown, bare(KeyCode::Down), settings(Navigate(Direction::Next))),
        row(SettingsNavigateUp, key('k'), settings(Navigate(Direction::Previous))),
        row(SettingsNavigateUp, bare(KeyCode::Up), settings(Navigate(Direction::Previous))),
        row(SettingsStepDown, key('h'), settings(Step(Direction::Previous))),
        row(SettingsStepDown, bare(KeyCode::Left), settings(Step(Direction::Previous))),
        row(SettingsStepUp, key('l'), settings(Step(Direction::Next))),
        row(SettingsStepUp, bare(KeyCode::Right), settings(Step(Direction::Next))),
    ]
}

fn confirm_trash_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::ConfirmTrash,
        vec![
            (letter('y'), confirm()),
            (plain(KeyCode::Enter), confirm()),
            (letter('n'), close()),
            (plain(KeyCode::Esc), close()),
            (letter('q'), close()),
        ],
    )
}

fn jump_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::JumpToTime,
        [
            vec![
                (plain(KeyCode::Esc), close()),
                (letter('q'), close()),
                (plain(KeyCode::Enter), confirm()),
            ],
            edits(text),
        ]
        .concat(),
    )
}

fn track_details_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::TrackDetails,
        vec![(KeyPattern::AnyKey, close())],
    )
}

fn servers_rows() -> Vec<KeyBinding> {
    let navigate = |direction| overlay(OverlayRequest::Navigate(direction));
    let row = row(KeyContext::Servers);
    [
        rows_in(
            KeyContext::Servers,
            vec![
                (letter('j'), navigate(Direction::Next)),
                (plain(KeyCode::Down), navigate(Direction::Next)),
                (letter('k'), navigate(Direction::Previous)),
                (plain(KeyCode::Up), navigate(Direction::Previous)),
                (plain(KeyCode::Enter), confirm()),
                (
                    letter('u'),
                    overlay(OverlayRequest::Open(OverlayName::AddServer)),
                ),
                (plain(KeyCode::Esc), close()),
                (letter('q'), close()),
            ],
        ),
        vec![
            row(
                Action::Reconnect,
                key('t'),
                overlay(OverlayRequest::Reconnect),
            ),
            row(
                Action::Delete,
                key('d'),
                overlay(OverlayRequest::Open(OverlayName::ConfirmRemove)),
            ),
        ],
    ]
    .concat()
}

fn confirm_remove_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::ConfirmRemove,
        vec![
            (plain(KeyCode::Enter), confirm()),
            (
                plain(KeyCode::Esc),
                overlay(OverlayRequest::Open(OverlayName::Servers)),
            ),
        ],
    )
}

pub(crate) fn rows() -> Vec<KeyBinding> {
    [
        text_prompt_rows(),
        search_rows(),
        help_rows(),
        history_rows(),
        settings_rows(),
        confirm_trash_rows(),
        jump_rows(),
        track_details_rows(),
        servers_rows(),
        confirm_remove_rows(),
    ]
    .concat()
}
