use strum::IntoEnumIterator;

use crate::domain::{cursor::Cursor, direction::Direction, index::ViewIndex};

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
}

pub(crate) fn cycled<T>(current: T, direction: Direction) -> T
where
    T: IntoEnumIterator + Copy + PartialEq,
    T::Iterator: ExactSizeIterator,
{
    let index = T::iter()
        .position(|variant| variant == current)
        .unwrap_or(0);
    T::iter()
        .nth(direction.wrapped(index, T::iter().len()))
        .unwrap_or(current)
}
