use crate::domain::direction::Direction;

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cursor {
    index: usize,
    len: usize,
}

impl Cursor {
    pub const fn at(len: usize, index: usize) -> Self {
        let index = if len == 0 {
            0
        } else if index < len {
            index
        } else {
            len - 1
        };
        Self { index, len }
    }

    pub fn new(len: usize) -> Self {
        Self::at(len, 0)
    }

    #[must_use]
    pub fn index(self) -> usize {
        self.index
    }

    #[must_use]
    pub fn len(self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    pub fn step(self, delta: isize) -> Self {
        if self.is_empty() {
            return self;
        }
        Self::at(self.len, clamped_step(self.index, self.len, delta))
    }

    pub(crate) fn page(self, rows: usize, direction: Direction) -> Self {
        let magnitude = isize::try_from(rows).unwrap_or(isize::MAX);
        self.step(magnitude.saturating_mul(direction.sign()))
    }

    pub(crate) fn first(self) -> Self {
        Self::at(self.len, 0)
    }

    pub fn last(self) -> Self {
        Self::at(self.len, self.len.saturating_sub(1))
    }

    pub fn resize(self, len: usize) -> Self {
        Self::at(len, self.index)
    }

    pub fn get<T>(self, items: &[T]) -> Option<&T> {
        items.get(self.index)
    }
}

fn clamped_step(index: usize, len: usize, delta: isize) -> usize {
    let moved = index.saturating_add_signed(delta);
    moved.min(len - 1)
}
