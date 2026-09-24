const RADIX: u32 = 10;

pub fn digits() -> impl Iterator<Item = u8> {
    (0..RADIX).filter_map(|digit| u8::try_from(digit).ok())
}

#[must_use]
pub(crate) fn digit_char(digit: u8) -> Option<char> {
    char::from_digit(u32::from(digit), RADIX)
}

#[cfg(test)]
mod tests {
    use crate::domain::digit::{digit_char, digits};

    #[test]
    fn the_run_is_zero_through_nine() {
        assert_eq!(
            digits().filter_map(digit_char).collect::<String>(),
            "0123456789"
        );
    }

    #[test]
    fn every_digit_in_the_run_spells_itself() {
        assert!(digits().all(|digit| digit_char(digit).is_some()));
    }

    #[test]
    fn nothing_past_the_run_spells_a_digit() {
        assert_eq!(digit_char(10), None);
        assert_eq!(digit_char(u8::MAX), None);
    }
}
