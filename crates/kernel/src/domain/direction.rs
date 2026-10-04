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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::direction::Direction;

    #[rstest]
    #[case::next(Direction::Next, (1, 3), 2)]
    #[case::next_wraps(Direction::Next, (2, 3), 0)]
    #[case::previous(Direction::Previous, (1, 3), 0)]
    #[case::previous_wraps(Direction::Previous, (0, 3), 2)]
    #[case::empty(Direction::Next, (0, 0), 0)]
    fn wrapped_stays_inside_the_length(
        #[case] direction: Direction,
        #[case] place: (usize, usize),
        #[case] expected: usize,
    ) {
        assert_eq!(direction.wrapped(place.0, place.1), expected);
    }
}
