use crate::{
    domain::{
        chord::{Chord, KeyPattern},
        key::{Key, KeyCode, Modifiers},
        keymap::{Action, KeyContext},
    },
    message::Message,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingSource {
    Configured,
    Default,
}

#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub pattern: KeyPattern,
    pub message: Message,
    pub action: Option<Action>,
    pub key_context: KeyContext,
    pub(crate) source: BindingSource,
}

pub(crate) fn key(character: char) -> Chord {
    bare(KeyCode::Char(character))
}

pub(crate) fn bare(code: KeyCode) -> Chord {
    Chord::Key(Key::plain(code))
}

pub(crate) fn shifted(code: KeyCode) -> Chord {
    Chord::Key(Key::new(code, Modifiers::SHIFT))
}

pub(crate) fn ctrl(character: char) -> Chord {
    Chord::Key(Key::ctrl(KeyCode::Char(character)))
}

const RADIX: u32 = 10;

pub(crate) fn digits() -> impl Iterator<Item = u8> {
    (0..RADIX).filter_map(|digit| u8::try_from(digit).ok())
}

#[must_use]
pub(crate) fn digit_char(digit: u8) -> Option<char> {
    char::from_digit(u32::from(digit), RADIX)
}

#[cfg(test)]
mod tests {
    use crate::update::keymap::chord::{digit_char, digits};

    #[test]
    fn the_run_is_zero_through_nine() {
        assert_eq!(
            digits().filter_map(digit_char).collect::<String>(),
            "0123456789"
        );
    }

    #[test]
    fn every_digit_in_the_run_spells_itself() {
        assert!(digits().all(|digit| digit_char(digit).is_some()));
    }

    #[test]
    fn nothing_past_the_run_spells_a_digit() {
        assert_eq!(digit_char(10), None);
        assert_eq!(digit_char(u8::MAX), None);
    }
}
