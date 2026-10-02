use kernel::{
    TextRequest,
    domain::{JumpDigits, TimecodeError},
    update::overlay::{JumpError, OverlayOutcome},
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
    TextRequest::Char('1'),
    Ok((digits("1", None), OverlayOutcome::default()))
)]
#[case::digit_appends(
    digits("1", None),
    TextRequest::Char('0'),
    Ok((digits("10", None), OverlayOutcome::default()))
)]
#[case::colon_appends(
    digits("1", None),
    TextRequest::Char(':'),
    Ok((digits("1:", None), OverlayOutcome::default()))
)]
#[case::digit_clears_a_stale_error(
    digits("5:", Some(TimecodeError::Malformed)),
    TextRequest::Char('3'),
    Ok((digits("5:3", None), OverlayOutcome::default()))
)]
#[case::letter_is_refused(
    digits("1", None),
    TextRequest::Char('a'),
    Err(JumpError::NotTimecodeChar)
)]
#[case::space_is_refused(
    digits("1", None),
    TextRequest::Char(' '),
    Err(JumpError::NotTimecodeChar)
)]
#[case::full_input_refuses_a_ninth_char(
    digits("1:02:034", None),
    TextRequest::Char('5'),
    Err(JumpError::Full)
)]
#[case::backspace_erases_and_clears_the_error(
    digits("5:", Some(TimecodeError::Malformed)),
    TextRequest::Backspace,
    Ok((digits("5", None), OverlayOutcome::default()))
)]
#[case::backspace_on_empty_stays_empty(
    digits("", None),
    TextRequest::Backspace,
    Ok((digits("", None), OverlayOutcome::default()))
)]
fn jump_cell(
    #[case] start: JumpDigits,
    #[case] message: TextRequest,
    #[case] expected: Cell,
) {
    cell(start, message, expected);
}
