use crate::{
    domain::{
        Action,
        CharSink,
        Chord,
        ChordPrefix,
        Direction,
        JumpDigits,
        Key,
        KeyCode,
        KeyContext,
        KeyPattern,
        Modifiers,
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
    update::keymap::{
        chord::{ActionRow, KeyBinding, KeyContextRow, KeyOutcome, bare, key},
        table::{digit_char, digits},
    },
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
        .map(|&chord| {
            ActionRow {
                action,
                chord,
                message: message.clone(),
                key_context: KeyContext::Settings,
            }
            .into()
        })
        .collect()
}

fn overlay(request: OverlayRequest) -> KeyOutcome {
    KeyOutcome::Message(Message::Overlay(request))
}

fn search(request: SearchRequest) -> KeyOutcome {
    overlay(OverlayRequest::Search(request))
}

fn edit(edit: SearchEdit) -> KeyOutcome {
    search(SearchRequest::Edit(edit))
}

fn history(request: HistoryRequest) -> KeyOutcome {
    overlay(OverlayRequest::History(request))
}

fn close() -> KeyOutcome {
    overlay(OverlayRequest::Close)
}

fn confirm() -> KeyOutcome {
    overlay(OverlayRequest::Confirm)
}

fn rows_in(
    key_context: KeyContext,
    rows: Vec<(KeyPattern, KeyOutcome)>,
) -> Vec<KeyBinding> {
    rows.into_iter()
        .map(|(pattern, outcome)| {
            KeyContextRow {
                key_context,
                pattern,
                outcome,
            }
            .into()
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
            (KeyPattern::AnyChar, KeyOutcome::TypeChar(CharSink::Text)),
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
            (KeyPattern::AnyChar, KeyOutcome::TypeChar(CharSink::Search)),
        ],
    )
}

fn help_rows() -> Vec<KeyBinding> {
    rows_in(
        KeyContext::Help,
        vec![
            (plain(KeyCode::Esc), close()),
            (letter('q'), KeyOutcome::Message(Message::Quit)),
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
    use SettingsRowRequest::{Activate, Adjust, Navigate};

    use crate::domain::Action::{
        SettingsActivate,
        SettingsAdjustDown,
        SettingsAdjustUp,
        SettingsClose,
        SettingsNavigateDown,
        SettingsNavigateUp,
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
            SettingsAdjustDown,
            &[key('h'), bare(KeyCode::Left)],
            &settings(Adjust(Direction::Previous)),
        ),
        settings_bindings(
            SettingsAdjustUp,
            &[key('l'), bare(KeyCode::Right)],
            &settings(Adjust(Direction::Next)),
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
