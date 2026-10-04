use kernel::{
    Cmd,
    domain::{JumpDigits, TimecodeError},
    update::{Unhandled, overlay::JumpDigitsMessage},
};
use rstest::rstest;

use crate::support::table::cell;

fn digits(input: &str, error: Option<TimecodeError>) -> JumpDigits {
    JumpDigits {
        input: input.to_string(),
        error,
    }
}

type Cell = crate::support::table::Cell<JumpDigits>;

#[rstest]
#[case::empty_digit_starts_the_time(
    digits("", None),
    JumpDigitsMessage::Char('1'),
    Ok((digits("1", None), Cmd::none()))
)]
#[case::digit_appends(
    digits("1", None),
    JumpDigitsMessage::Char('0'),
    Ok((digits("10", None), Cmd::none()))
)]
#[case::colon_appends(
    digits("1", None),
    JumpDigitsMessage::Char(':'),
    Ok((digits("1:", None), Cmd::none()))
)]
#[case::digit_clears_a_stale_error(
    digits("5:", Some(TimecodeError::Malformed)),
    JumpDigitsMessage::Char('3'),
    Ok((digits("5:3", None), Cmd::none()))
)]
#[case::letter_is_refused(
    digits("1", None),
    JumpDigitsMessage::Char('a'),
    Err(Unhandled)
)]
#[case::space_is_refused(
    digits("1", None),
    JumpDigitsMessage::Char(' '),
    Err(Unhandled)
)]
#[case::full_input_refuses_a_ninth_char(
    digits("1:02:034", None),
    JumpDigitsMessage::Char('5'),
    Err(Unhandled)
)]
#[case::backspace_erases_and_clears_the_error(
    digits("5:", Some(TimecodeError::Malformed)),
    JumpDigitsMessage::Backspace,
    Ok((digits("5", None), Cmd::none()))
)]
#[case::backspace_on_empty_stays_empty(
    digits("", None),
    JumpDigitsMessage::Backspace,
    Ok((digits("", None), Cmd::none()))
)]
fn jump_cell(
    #[case] start: JumpDigits,
    #[case] message: JumpDigitsMessage,
    #[case] expected: Cell,
) {
    cell(start, message, expected);
}
