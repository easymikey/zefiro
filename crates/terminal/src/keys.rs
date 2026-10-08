use crossterm::event::{KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
use kernel::domain::key::{Key, KeyCode, KeyPress, Modifiers};

#[must_use]
pub fn key_press(key_event: KeyEvent) -> Option<KeyPress> {
    let typed = key(key_event)?;
    let key = Key {
        code: match typed.code {
            KeyCode::Char(character) => KeyCode::Char(qwerty_char(character)),
            other @ (KeyCode::Enter
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
            | KeyCode::PageDown) => other,
        },
        modifiers: typed.modifiers,
    };
    Some(KeyPress { key, typed })
}

const JCUKEN_LAYOUT: &str =
    "йцукенгшщзхъфывапролджэячсмитьбю.ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";
const QWERTY_LAYOUT: &str =
    "qwertyuiop[]asdfghjkl;'zxcvbnm,./QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?";

fn qwerty_char(character: char) -> char {
    Some(character)
        .filter(|typed| !typed.is_ascii())
        .and_then(|typed| JCUKEN_LAYOUT.chars().position(|cyrillic| cyrillic == typed))
        .and_then(|index| QWERTY_LAYOUT.chars().nth(index))
        .unwrap_or(character)
}

fn key(key_event: KeyEvent) -> Option<Key> {
    let code = match key_event.code {
        CrosstermCode::Char(character) => KeyCode::Char(character),
        CrosstermCode::Enter => KeyCode::Enter,
        CrosstermCode::Esc => KeyCode::Esc,
        CrosstermCode::Backspace => KeyCode::Backspace,
        CrosstermCode::Up => KeyCode::Up,
        CrosstermCode::Down => KeyCode::Down,
        CrosstermCode::Left => KeyCode::Left,
        CrosstermCode::Right => KeyCode::Right,
        CrosstermCode::Home => KeyCode::Home,
        CrosstermCode::End => KeyCode::End,
        CrosstermCode::Tab | CrosstermCode::BackTab => KeyCode::Tab,
        CrosstermCode::PageUp => KeyCode::PageUp,
        CrosstermCode::PageDown => KeyCode::PageDown,
        CrosstermCode::Delete
        | CrosstermCode::Insert
        | CrosstermCode::F(_)
        | CrosstermCode::Null
        | CrosstermCode::CapsLock
        | CrosstermCode::ScrollLock
        | CrosstermCode::NumLock
        | CrosstermCode::PrintScreen
        | CrosstermCode::Pause
        | CrosstermCode::Menu
        | CrosstermCode::KeypadBegin
        | CrosstermCode::Media(_)
        | CrosstermCode::Modifier(_) => return None,
    };
    let modifiers = [
        (KeyModifiers::CONTROL, Modifiers::CTRL),
        (KeyModifiers::ALT, Modifiers::ALT),
        (KeyModifiers::SUPER, Modifiers::SUPER),
        (KeyModifiers::SHIFT, Modifiers::SHIFT),
    ]
    .into_iter()
    .filter(|(held, _)| {
        key_event.modifiers.contains(*held)
            && (*held != KeyModifiers::SHIFT || is_shift_reportable(key_event.code))
    })
    .fold(
        if key_event.code == CrosstermCode::BackTab {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        },
        |modifiers, (_, mapped)| modifiers.with(mapped),
    );
    Some(Key { code, modifiers })
}

fn is_shift_reportable(code: CrosstermCode) -> bool {
    !matches!(code, CrosstermCode::Char(_))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::domain::key::{KeyCode, Modifiers};
    use rstest::rstest;

    use crate::keys::{key, key_press};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Shift {
        Reported,
        NotReported,
    }

    struct ShiftRow {
        code: CrosstermCode,
        modifiers: KeyModifiers,
        expected_code: KeyCode,
        expected_shift: Shift,
    }

    #[rstest]
    #[case::shifted_uppercase_char_reports_shift_false(ShiftRow {
        code: CrosstermCode::Char('H'),
        modifiers: KeyModifiers::SHIFT,
        expected_code: KeyCode::Char('H'),
        expected_shift: Shift::NotReported,
    })]
    #[case::shifted_arrow_reports_shift_true(ShiftRow {
        code: CrosstermCode::Up,
        modifiers: KeyModifiers::SHIFT,
        expected_code: KeyCode::Up,
        expected_shift: Shift::Reported,
    })]
    #[case::unshifted_lowercase_char_unchanged(ShiftRow {
        code: CrosstermCode::Char('h'),
        modifiers: KeyModifiers::NONE,
        expected_code: KeyCode::Char('h'),
        expected_shift: Shift::NotReported,
    })]
    #[case::bare_back_tab_reports_tab_with_shift(ShiftRow {
        code: CrosstermCode::BackTab,
        modifiers: KeyModifiers::NONE,
        expected_code: KeyCode::Tab,
        expected_shift: Shift::Reported,
    })]
    fn shift_is_reported_only_when_the_code_carries_no_built_in_case(
        #[case] row: ShiftRow,
    ) {
        let ShiftRow {
            code,
            modifiers,
            expected_code,
            expected_shift,
        } = row;
        let key_event = KeyEvent::new(code, modifiers);
        let converted = key(key_event).map(|key| {
            let shift = if key.modifiers.contains(Modifiers::SHIFT) {
                Shift::Reported
            } else {
                Shift::NotReported
            };
            (key.code, shift)
        });
        assert_eq!(converted, Some((expected_code, expected_shift)));
    }

    #[rstest]
    #[case('й', 'q')]
    #[case('х', '[')]
    #[case('ъ', ']')]
    #[case('ж', ';')]
    #[case('э', '\'')]
    #[case('б', ',')]
    #[case('ю', '.')]
    #[case('.', '.')]
    #[case('Х', '{')]
    #[case('Ж', ':')]
    #[case('Э', '"')]
    #[case('Б', '<')]
    #[case('Ю', '>')]
    fn layout_translation_maps_the_key_and_types_the_char_verbatim(
        #[case] ru: char,
        #[case] en: char,
    ) {
        let key_event = KeyEvent::new(CrosstermCode::Char(ru), KeyModifiers::NONE);
        let translated =
            key_press(key_event).map(|press| (press.key.code, press.typed.code));
        assert_eq!(translated, Some((KeyCode::Char(en), KeyCode::Char(ru))));
    }

    #[test]
    fn conversion_preserves_control_modifier_after_normalization() {
        let key_event = KeyEvent::new(CrosstermCode::Char('л'), KeyModifiers::CONTROL);
        let key = key_press(key_event).unwrap().key;
        assert_eq!(key.code, KeyCode::Char('k'));
        assert!(key.modifiers.contains(Modifiers::CTRL));
    }
}
