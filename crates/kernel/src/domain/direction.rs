#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Next,
    Previous,
}

impl Direction {
    #[must_use]
    pub(crate) const fn sign(self) -> isize {
        match self {
            Direction::Next => 1,
            Direction::Previous => -1,
        }
    }

    #[must_use]
    pub(crate) fn wrapped(self, current: usize, len: usize) -> usize {
        let len = len.max(1);
        match self {
            Direction::Next => current.saturating_add(1) % len,
            Direction::Previous => (current % len + len - 1) % len,
        }
    }
}
