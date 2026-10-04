use strum::IntoEnumIterator;

use crate::domain::{ViewIndex, cursor::Cursor, direction::Direction};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorOver<T> {
    pub cursor: Cursor,
    pub content: T,
}

impl<T> CursorOver<T> {
    pub fn new(content: T, len: usize) -> Self {
        Self {
            cursor: Cursor::new(len),
            content,
        }
    }

    pub fn selected(&self) -> ViewIndex {
        ViewIndex::new(self.cursor.index())
    }

    pub fn resize(&mut self, len: usize) {
        self.cursor = self.cursor.resize(len);
    }

    pub(crate) fn navigate(&mut self, direction: Direction) {
        self.cursor = self.cursor.step(direction.sign());
    }
}

pub(crate) fn cycled<T>(current: T, direction: Direction) -> T
where
    T: IntoEnumIterator + Copy + PartialEq,
{
    let variants: Vec<T> = T::iter().collect();
    let index = variants
        .iter()
        .position(|variant| *variant == current)
        .unwrap_or(0);
    let next = direction.wrapped(index, variants.len());
    variants.get(next).copied().unwrap_or(current)
}
