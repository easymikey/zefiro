use crate::{
    domain::{
        chord::{Chord, ChordPrefix, KeyPattern},
        direction::Direction,
        key::{Key, KeyCode, Modifiers},
        keymap::{Action, KeyContext},
        overlay::JumpDigits,
    },
    message::{
        HistoryRequest,
        Message,
        OverlayRequest,
        SearchEdit,
        SearchRequest,
        SettingsRowRequest,
        TextRequest,
    },
    update::keymap::chord::{BindingSource, KeyBinding, bare, digit_char, digits, key},
};

fn plain(code: KeyCode) -> KeyPattern {
    KeyPattern::Chord(bare(code))
}

fn letter(character: char) -> KeyPattern {
    KeyPattern::Chord(key(character))
}

fn held(modifiers: Modifiers, code: KeyCode) -> KeyPattern {
    KeyPattern::Chord(Chord::Key(Key::new(code, modifiers)))
}

fn settings_bindings(
    action: Action,
    chords: &[Chord],
    message: &Message,
) -> Vec<KeyBinding> {
    chords
        .iter()
        .map(|&chord| KeyBinding {
            pattern: KeyPattern::Chord(chord),
            message: message.clone(),
            action: Some(action),
            key_context: KeyContext::Settings,
            source: BindingSource::Default,
        })
        .collect()
}

fn overlay(request: OverlayRequest) -> Message {
    Message::Overlay(request)
}

fn search(request: SearchRequest) -> Message {
    overlay(OverlayRequest::Search(request))
}

fn edit(edit: SearchEdit) -> Message {
    search(SearchRequest::Edit(edit))
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
            source: BindingSource::Default,
        })
        .collect()
}

fn text_prompt_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::TextPrompt,
        vec![
            (plain(KeyCode::Enter), confirm()),
            (plain(KeyCode::Esc), close()),
            (
                plain(KeyCode::Backspace),
                overlay(OverlayRequest::Text(TextRequest::Backspace)),
            ),
        ],
    )
}

fn search_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::Search,
        vec![
            (
                held(Modifiers::CTRL, KeyCode::Char('u')),
                edit(SearchEdit::Clear),
            ),
            (
                held(Modifiers::CTRL, KeyCode::Char('w')),
                edit(SearchEdit::DeleteWord),
            ),
            (
                held(Modifiers::ALT, KeyCode::Backspace),
                edit(SearchEdit::DeleteWord),
            ),
            (
                held(Modifiers::SUPER, KeyCode::Backspace),
                edit(SearchEdit::Clear),
            ),
            (plain(KeyCode::Esc), close()),
            (plain(KeyCode::Enter), confirm()),
            (plain(KeyCode::Backspace), edit(SearchEdit::Backspace)),
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
    )
}

fn help_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::Help,
        vec![
            (plain(KeyCode::Esc), close()),
            (letter('q'), Message::Quit),
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
                    prefix: ChordPrefix::G,
                    key: ChordPrefix::G.key(),
                }),
                history(HistoryRequest::Top),
            ),
            (plain(KeyCode::Esc), close()),
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
            (letter('G'), history(HistoryRequest::Bottom)),
        ],
    )
}

fn settings_rows() -> Vec<KeyBinding> {
    use SettingsRowRequest::{Activate, Navigate, Step};

    use crate::domain::keymap::Action::{
        SettingsActivate,
        SettingsClose,
        SettingsNavigateDown,
        SettingsNavigateUp,
        SettingsStepDown,
        SettingsStepUp,
    };
    let settings = |request| Message::Overlay(OverlayRequest::Settings(request));
    [
        settings_bindings(
            SettingsClose,
            &[bare(KeyCode::Esc)],
            &Message::Overlay(OverlayRequest::Close),
        ),
        settings_bindings(
            SettingsActivate,
            &[bare(KeyCode::Enter), key(' ')],
            &settings(Activate),
        ),
        settings_bindings(
            SettingsNavigateDown,
            &[key('j'), bare(KeyCode::Down)],
            &settings(Navigate(Direction::Next)),
        ),
        settings_bindings(
            SettingsNavigateUp,
            &[key('k'), bare(KeyCode::Up)],
            &settings(Navigate(Direction::Previous)),
        ),
        settings_bindings(
            SettingsStepDown,
            &[key('h'), bare(KeyCode::Left)],
            &settings(Step(Direction::Previous)),
        ),
        settings_bindings(
            SettingsStepUp,
            &[key('l'), bare(KeyCode::Right)],
            &settings(Step(Direction::Next)),
        ),
    ]
    .concat()
}

fn confirm_delete_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::ConfirmDelete,
        vec![
            (letter('y'), confirm()),
            (plain(KeyCode::Enter), confirm()),
            (letter('n'), close()),
            (plain(KeyCode::Esc), close()),
        ],
    )
}

fn jump_rows() -> Vec<KeyBinding> {
    let jump_keys = digits()
        .filter_map(digit_char)
        .chain([JumpDigits::SEPARATOR])
        .map(|character| {
            (
                letter(character),
                overlay(OverlayRequest::Jump(TextRequest::Char(character))),
            )
        });
    let rows = [
        (plain(KeyCode::Esc), close()),
        (plain(KeyCode::Enter), confirm()),
        (
            plain(KeyCode::Backspace),
            overlay(OverlayRequest::Jump(TextRequest::Backspace)),
        ),
    ]
    .into_iter()
    .chain(jump_keys)
    .collect();
    rows_in(KeyContext::JumpToTime, rows)
}

fn track_details_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::TrackDetails,
        vec![(KeyPattern::AnyKey, close())],
    )
}

pub(crate) fn rows() -> Vec<KeyBinding> {
    [
        text_prompt_rows(),
        search_rows(),
        help_rows(),
        history_rows(),
        settings_rows(),
        confirm_delete_rows(),
        jump_rows(),
        track_details_rows(),
    ]
    .concat()
}
