use strum::IntoEnumIterator;

use crate::domain::cursor::Cursor;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorOver<T> {
    pub cursor: Cursor,
    pub rows: T,
}

impl<T> CursorOver<T> {
    pub fn new(rows: T, len: usize) -> Self {
        Self {
            cursor: Cursor::new(len),
            rows,
        }
    }

    pub fn selected(&self) -> usize {
        self.cursor.index()
    }

    pub fn resize(&mut self, len: usize) {
        self.cursor = self.cursor.resize(len);
    }

    pub(crate) fn select(&mut self, index: usize) {
        self.cursor = Cursor::with_len(self.cursor.len()).at(index);
    }

    pub(crate) fn navigate(&mut self, motion: ListMotion) {
        self.cursor = match motion {
            ListMotion::Up => self.cursor.step(-1),
            ListMotion::Down => self.cursor.step(1),
            ListMotion::First => self.cursor.first(),
            ListMotion::Last => self.cursor.last(),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    Up,
    Down,
}

impl From<Nudge> for ListMotion {
    fn from(nudge: Nudge) -> Self {
        match nudge {
            Nudge::Up => ListMotion::Up,
            Nudge::Down => ListMotion::Down,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListMotion {
    Up,
    Down,
    First,
    Last,
}

pub(crate) fn cycled<T>(current: T, nudge: Nudge) -> T
where
    T: IntoEnumIterator + Copy + PartialEq,
{
    let variants: Vec<T> = T::iter().collect();
    let index = variants
        .iter()
        .position(|variant| *variant == current)
        .unwrap_or(0);
    let last = variants.len().saturating_sub(1);
    let next = match nudge {
        Nudge::Up if index >= last => 0,
        Nudge::Up => index + 1,
        Nudge::Down if index == 0 => last,
        Nudge::Down => index - 1,
    };
    variants.get(next).copied().unwrap_or(current)
}
