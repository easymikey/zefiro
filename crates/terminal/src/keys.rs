use crossterm::event::{KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
use kernel::domain::key::{Key, KeyCode, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutTranslation {
    Applied,
    Verbatim,
}

#[must_use]
pub fn from_event(event: KeyEvent, translation: LayoutTranslation) -> Option<Key> {
    let event = match translation {
        LayoutTranslation::Applied => normalized(event),
        LayoutTranslation::Verbatim => event,
    };
    to_key(event)
}

const JCUKEN_LAYOUT: &str =
    "йцукенгшщзхъфывапролджэячсмитьбю.ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";
const QWERTY_LAYOUT: &str =
    "qwertyuiop[]asdfghjkl;'zxcvbnm,./QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?";

fn normalized(event: KeyEvent) -> KeyEvent {
    if let CrosstermCode::Char(character) = event.code {
        KeyEvent {
            code: CrosstermCode::Char(qwerty_char(character)),
            ..event
        }
    } else {
        event
    }
}

fn qwerty_char(character: char) -> char {
    Some(character)
        .filter(|typed| !typed.is_ascii())
        .and_then(|typed| JCUKEN_LAYOUT.chars().position(|cyrillic| cyrillic == typed))
        .and_then(|index| QWERTY_LAYOUT.chars().nth(index))
        .unwrap_or(character)
}

fn to_key(event: KeyEvent) -> Option<Key> {
    let code = match event.code {
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
        CrosstermCode::Tab => KeyCode::Tab,
        CrosstermCode::PageUp => KeyCode::PageUp,
        CrosstermCode::PageDown => KeyCode::PageDown,
        CrosstermCode::BackTab
        | CrosstermCode::Delete
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
        event.modifiers.contains(*held)
            && (*held != KeyModifiers::SHIFT || reportable_shift(event.code))
    })
    .fold(Modifiers::NONE, |modifiers, (_, mapped)| {
        modifiers.with(mapped)
    });
    Some(Key { code, modifiers })
}

fn reportable_shift(code: CrosstermCode) -> bool {
    !matches!(code, CrosstermCode::Char(_))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::domain::key::{KeyCode, Modifiers};
    use rstest::rstest;

    use crate::keys::{LayoutTranslation, from_event, to_key};

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
    fn shift_is_reported_only_when_the_code_carries_no_built_in_case(
        #[case] row: ShiftRow,
    ) {
        let ShiftRow {
            code,
            modifiers,
            expected_code,
            expected_shift,
        } = row;
        let event = KeyEvent::new(code, modifiers);
        let converted = to_key(event).map(|key| {
            let shift = if key.modifiers.contains(Modifiers::SHIFT) {
                Shift::Reported
            } else {
                Shift::NotReported
            };
            (key.code, shift)
        });
        assert_eq!(converted, Some((expected_code, expected_shift)));
    }

    #[test]
    fn layout_translation_applied_maps_ru_char_verbatim_keeps_it() {
        let event = KeyEvent::new(CrosstermCode::Char('й'), KeyModifiers::NONE);
        let applied = from_event(event, LayoutTranslation::Applied).map(|key| key.code);
        assert_eq!(applied, Some(KeyCode::Char('q')));

        let verbatim =
            from_event(event, LayoutTranslation::Verbatim).map(|key| key.code);
        assert_eq!(verbatim, Some(KeyCode::Char('й')));
    }

    #[rstest::rstest]
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
    #[case(',', ',')]
    fn layout_translation_covers_the_punctuation_keys(
        #[case] ru: char,
        #[case] en: char,
    ) {
        let event = KeyEvent::new(CrosstermCode::Char(ru), KeyModifiers::NONE);
        let applied = from_event(event, LayoutTranslation::Applied).map(|key| key.code);
        assert_eq!(applied, Some(KeyCode::Char(en)));
    }

    #[test]
    fn conversion_preserves_control_modifier_after_normalization() {
        let event = KeyEvent::new(CrosstermCode::Char('л'), KeyModifiers::CONTROL);
        let key = from_event(event, LayoutTranslation::Applied).unwrap();
        assert_eq!(key.code, KeyCode::Char('k'));
        assert!(key.modifiers.contains(Modifiers::CTRL));
    }
}
