#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);
    pub const CTRL: Modifiers = Modifiers(1 << 0);
    pub const ALT: Modifiers = Modifiers(1 << 1);
    pub const SUPER: Modifiers = Modifiers(1 << 2);
    pub const SHIFT: Modifiers = Modifiers(1 << 3);

    #[must_use]
    pub fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Modifiers) -> Self {
        Modifiers(self.0 | other.0)
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl Key {
    pub fn plain(code: KeyCode) -> Self {
        Key {
            code,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn ctrl(code: KeyCode) -> Self {
        Key {
            code,
            modifiers: Modifiers::CTRL,
        }
    }

    pub fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        Key { code, modifiers }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub typed: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Tab,
    PageUp,
    PageDown,
}
