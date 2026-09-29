use crate::{
    domain::{
        Action,
        CharSink,
        Chord,
        ChordPrefix,
        JumpDigits,
        Key,
        KeyCode,
        KeyContext,
        KeyPattern,
        Modifiers,
        Nudge,
        digit_char,
        digits,
    },
    message::{
        HistoryRequest,
        JumpRequest,
        Message,
        OverlayRequest,
        SearchEdit,
        SearchRequest,
        SettingsRowRequest,
        TextRequest,
    },
    update::keymap::chord::{ActionRow, KeyBinding, KeyContextRow, KeyOutcome},
};

fn plain(code: KeyCode) -> KeyPattern {
    KeyPattern::Chord(Chord::Key(Key::plain(code)))
}

fn letter(character: char) -> KeyPattern {
    plain(KeyCode::Char(character))
}

fn held(modifiers: Modifiers, code: KeyCode) -> KeyPattern {
    KeyPattern::Chord(Chord::Key(Key::new(code, modifiers)))
}

fn key_chord(code: KeyCode) -> Chord {
    Chord::Key(Key::plain(code))
}

fn letter_chord(character: char) -> Chord {
    key_chord(KeyCode::Char(character))
}

fn settings_action_row(
    (action, chord, message): (Action, Chord, Message),
) -> KeyBinding {
    ActionRow {
        action,
        chord,
        message,
        key_context: KeyContext::Settings,
    }
    .into()
}

fn settings_bindings(
    action: Action,
    chords: &[Chord],
    message: &Message,
) -> Vec<KeyBinding> {
    chords
        .iter()
        .map(|&chord| settings_action_row((action, chord, message.clone())))
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

type KeyContextRows = (KeyContext, Vec<(KeyPattern, KeyOutcome)>);

fn rows_in((key_context, rows): KeyContextRows) -> Vec<KeyBinding> {
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
    rows_in((
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
    ))
}

fn search_rows() -> Vec<KeyBinding> {
    rows_in((
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
                search(SearchRequest::Navigate(Nudge::Down)),
            ),
            (
                plain(KeyCode::Up),
                search(SearchRequest::Navigate(Nudge::Up)),
            ),
            (plain(KeyCode::Tab), search(SearchRequest::Enqueue)),
            (KeyPattern::AnyChar, KeyOutcome::TypeChar(CharSink::Search)),
        ],
    ))
}

fn help_rows() -> Vec<KeyBinding> {
    rows_in((
        KeyContext::Help,
        vec![
            (plain(KeyCode::Esc), close()),
            (letter('q'), KeyOutcome::Message(Message::Quit)),
            (held(Modifiers::CTRL, KeyCode::Char('k')), close()),
        ],
    ))
}

fn history_rows() -> Vec<KeyBinding> {
    rows_in((
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
            (letter('j'), history(HistoryRequest::Navigate(Nudge::Down))),
            (
                plain(KeyCode::Down),
                history(HistoryRequest::Navigate(Nudge::Down)),
            ),
            (letter('k'), history(HistoryRequest::Navigate(Nudge::Up))),
            (
                plain(KeyCode::Up),
                history(HistoryRequest::Navigate(Nudge::Up)),
            ),
            (letter('G'), history(HistoryRequest::Bottom)),
        ],
    ))
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
            &[key_chord(KeyCode::Esc)],
            &Message::Overlay(OverlayRequest::Close),
        ),
        settings_bindings(
            SettingsActivate,
            &[key_chord(KeyCode::Enter), letter_chord(' ')],
            &settings(Activate),
        ),
        settings_bindings(
            SettingsNavigateDown,
            &[letter_chord('j'), key_chord(KeyCode::Down)],
            &settings(Navigate(Nudge::Down)),
        ),
        settings_bindings(
            SettingsNavigateUp,
            &[letter_chord('k'), key_chord(KeyCode::Up)],
            &settings(Navigate(Nudge::Up)),
        ),
        settings_bindings(
            SettingsAdjustDown,
            &[letter_chord('h'), key_chord(KeyCode::Left)],
            &settings(Adjust(Nudge::Down)),
        ),
        settings_bindings(
            SettingsAdjustUp,
            &[letter_chord('l'), key_chord(KeyCode::Right)],
            &settings(Adjust(Nudge::Up)),
        ),
    ]
    .concat()
}

fn confirm_delete_rows() -> Vec<KeyBinding> {
    rows_in((
        KeyContext::ConfirmDelete,
        vec![
            (letter('y'), confirm()),
            (plain(KeyCode::Enter), confirm()),
            (letter('n'), close()),
            (plain(KeyCode::Esc), close()),
        ],
    ))
}

fn jump_rows() -> Vec<KeyBinding> {
    let mut rows = vec![
        (plain(KeyCode::Esc), close()),
        (plain(KeyCode::Enter), confirm()),
        (
            plain(KeyCode::Backspace),
            overlay(OverlayRequest::Jump(JumpRequest::Backspace)),
        ),
    ];
    let jump_keys = digits()
        .filter_map(digit_char)
        .chain([JumpDigits::SEPARATOR]);
    rows.extend(jump_keys.map(|character| {
        (
            letter(character),
            overlay(OverlayRequest::Jump(JumpRequest::Char(character))),
        )
    }));
    rows_in((KeyContext::JumpToTime, rows))
}

fn track_details_rows() -> Vec<KeyBinding> {
    rows_in((
        KeyContext::TrackDetails,
        vec![(KeyPattern::AnyKey, close())],
    ))
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
