use kernel::{
    JumpRequest,
    domain::{JumpDigits, TimecodeError},
    update::overlay::{JumpRejection, OverlayEffect},
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
    JumpRequest::Char('1'),
    Ok((digits("1", None), OverlayEffect::default()))
)]
#[case::digit_appends(
    digits("1", None),
    JumpRequest::Char('0'),
    Ok((digits("10", None), OverlayEffect::default()))
)]
#[case::colon_appends(
    digits("1", None),
    JumpRequest::Char(':'),
    Ok((digits("1:", None), OverlayEffect::default()))
)]
#[case::digit_clears_a_stale_error(
    digits("5:", Some(TimecodeError::Malformed)),
    JumpRequest::Char('3'),
    Ok((digits("5:3", None), OverlayEffect::default()))
)]
#[case::letter_is_refused(
    digits("1", None),
    JumpRequest::Char('a'),
    Err(JumpRejection::NotTimecodeChar)
)]
#[case::space_is_refused(
    digits("1", None),
    JumpRequest::Char(' '),
    Err(JumpRejection::NotTimecodeChar)
)]
#[case::full_input_refuses_a_ninth_char(
    digits("1:02:034", None),
    JumpRequest::Char('5'),
    Err(JumpRejection::Full)
)]
#[case::backspace_erases_and_clears_the_error(
    digits("5:", Some(TimecodeError::Malformed)),
    JumpRequest::Backspace,
    Ok((digits("5", None), OverlayEffect::default()))
)]
#[case::backspace_on_empty_stays_empty(
    digits("", None),
    JumpRequest::Backspace,
    Ok((digits("", None), OverlayEffect::default()))
)]
fn jump_cell(
    #[case] start: JumpDigits,
    #[case] message: JumpRequest,
    #[case] expected: Cell,
) {
    cell(start, message, expected);
}
