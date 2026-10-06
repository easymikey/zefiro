use crate::domain::{overlay::Accepts, time::TimecodeError};

const SEPARATOR: char = ':';

impl Accepts for TimecodeError {
    const MAX_LEN: usize = 8;

    fn accepts(character: char) -> bool {
        character.is_ascii_digit() || character == SEPARATOR
    }
}
