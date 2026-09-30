use crate::domain::direction::Direction;

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cursor {
    index: usize,
    len: usize,
}

#[bon::bon]
impl Cursor {
    #[builder(
        builder_type(vis = "pub"),
        state_mod(vis = "pub"),
        start_fn(name = with_len, vis = "pub"),
        finish_fn(name = at, vis = "pub")
    )]
    fn clamped(
        #[builder(start_fn)] len: usize,
        #[builder(finish_fn)] index: usize,
    ) -> Self {
        let index = if len == 0 { 0 } else { index.min(len - 1) };
        Self { index, len }
    }

    pub fn new(len: usize) -> Self {
        Self::with_len(len).at(0)
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
        Self::with_len(self.len).at(clamped_step(self.index, self.len, delta))
    }

    pub(crate) fn page(self, rows: usize, direction: Direction) -> Self {
        let magnitude = isize::try_from(rows).unwrap_or(isize::MAX);
        self.step(magnitude.saturating_mul(direction.sign()))
    }

    pub fn first(self) -> Self {
        Self::with_len(self.len).at(0)
    }

    pub fn last(self) -> Self {
        Self::with_len(self.len).at(self.len.saturating_sub(1))
    }

    pub fn resize(self, len: usize) -> Self {
        Self::with_len(len).at(self.index)
    }

    pub fn get<T>(self, items: &[T]) -> Option<&T> {
        items.get(self.index)
    }
}

fn clamped_step(index: usize, len: usize, delta: isize) -> usize {
    let moved = index.checked_add_signed(delta).unwrap_or(0);
    moved.min(len - 1)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{cursor::Cursor, direction::Direction};

    struct PageRow {
        index: usize,
        len: usize,
        rows: usize,
        direction: Direction,
        expected_index: usize,
    }

    #[rstest]
    #[case(PageRow { index: 3, len: 5, rows: 10, direction: Direction::Next, expected_index: 4 })]
    #[case(PageRow { index: 1, len: 5, rows: 10, direction: Direction::Previous, expected_index: 0 })]
    fn page_clamps_at_the_end(#[case] row: PageRow) {
        let cursor = Cursor::with_len(row.len)
            .at(row.index)
            .page(row.rows, row.direction);
        assert_eq!(cursor.index(), row.expected_index);
    }
}
