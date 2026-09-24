use std::fmt;

use crate::domain::{Key, KeyCode, Modifiers};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid key chord `{spelling}`")]
pub struct ChordParseError {
    pub spelling: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chord {
    Key(Key),
    Sequence { prefix: ChordPrefix, key: Key },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordPrefix {
    G,
}

impl fmt::Display for ChordPrefix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::G => formatter.write_str("g"),
        }
    }
}

impl ChordPrefix {
    pub(crate) fn key(self) -> Key {
        match self {
            Self::G => Key::plain(KeyCode::Char('g')),
        }
    }

    #[must_use]
    pub(crate) fn from_key(key: Key) -> Option<Self> {
        [Self::G].into_iter().find(|&prefix| prefix.key() == key)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sequence { prefix, key } => {
                write!(formatter, "{prefix}")?;
                Self::Key(*key).fmt(formatter)
            }
            Self::Key(Key { code, modifiers }) => {
                let ctrl = modifiers.contains(Modifiers::CTRL);
                if ctrl {
                    formatter.write_str("Ctrl+")?;
                }
                if modifiers.contains(Modifiers::ALT) {
                    formatter.write_str("Alt+")?;
                }
                if modifiers.contains(Modifiers::SUPER) {
                    formatter.write_str("Super+")?;
                }
                if modifiers.contains(Modifiers::SHIFT) {
                    formatter.write_str("Shift+")?;
                }
                match code {
                    KeyCode::Char(' ') => formatter.write_str("Space"),
                    KeyCode::Char(character) if ctrl => {
                        write!(formatter, "{}", character.to_ascii_uppercase())
                    }
                    KeyCode::Char(character) => write!(formatter, "{character}"),
                    KeyCode::Enter => formatter.write_str("Enter"),
                    KeyCode::Esc => formatter.write_str("Esc"),
                    KeyCode::Backspace => formatter.write_str("Backspace"),
                    KeyCode::Up => formatter.write_str("↑"),
                    KeyCode::Down => formatter.write_str("↓"),
                    KeyCode::Left => formatter.write_str("←"),
                    KeyCode::Right => formatter.write_str("→"),
                    KeyCode::Home => formatter.write_str("Home"),
                    KeyCode::End => formatter.write_str("End"),
                    KeyCode::Tab => formatter.write_str("Tab"),
                    KeyCode::PageUp => formatter.write_str("PgUp"),
                    KeyCode::PageDown => formatter.write_str("PgDn"),
                }
            }
        }
    }
}

impl std::str::FromStr for Chord {
    type Err = ChordParseError;

    fn from_str(spelling: &str) -> Result<Self, ChordParseError> {
        let (ctrl, spelling) = spelling
            .strip_prefix("ctrl+")
            .map_or((false, spelling), |rest| (true, rest));
        let (shift, spelling) = spelling
            .strip_prefix("shift+")
            .map_or((false, spelling), |rest| (true, rest));
        if spelling == "gg" && !ctrl && !shift {
            return Ok(Self::Sequence {
                prefix: ChordPrefix::G,
                key: ChordPrefix::G.key(),
            });
        }
        let code = match spelling {
            "space" => KeyCode::Char(' '),
            "enter" | "Enter" => KeyCode::Enter,
            "esc" | "Esc" => KeyCode::Esc,
            "backspace" | "Backspace" => KeyCode::Backspace,
            "up" | "↑" => KeyCode::Up,
            "down" | "↓" => KeyCode::Down,
            "left" | "←" => KeyCode::Left,
            "right" | "→" => KeyCode::Right,
            "home" | "Home" => KeyCode::Home,
            "end" | "End" => KeyCode::End,
            "tab" | "Tab" => KeyCode::Tab,
            "pageup" | "pgup" | "PgUp" => KeyCode::PageUp,
            "pagedown" | "pgdn" | "PgDn" => KeyCode::PageDown,
            other => {
                KeyCode::Char(other.parse::<char>().map_err(|_| ChordParseError {
                    spelling: spelling.to_string(),
                })?)
            }
        };
        let mut modifiers = Modifiers::NONE;
        if ctrl {
            modifiers = modifiers.with(Modifiers::CTRL);
        }
        if shift {
            modifiers = modifiers.with(Modifiers::SHIFT);
        }
        Ok(Self::Key(Key { code, modifiers }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyPattern {
    Chord(Chord),
    AnyChar,
    AnyKey,
}

impl KeyPattern {
    #[must_use]
    pub fn chord(self) -> Option<Chord> {
        match self {
            Self::Chord(chord) => Some(chord),
            Self::AnyChar | Self::AnyKey => None,
        }
    }
}

impl fmt::Display for KeyPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chord(chord) => chord.fmt(formatter),
            Self::AnyChar => formatter.write_str("any character"),
            Self::AnyKey => formatter.write_str("any key"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharSink {
    Text,
    Search,
}
