use kernel::{
    cmd::Cmd,
    domain::{
        overlay::{Overlay, TextEntry},
        time::TimecodeError,
    },
    message::TextRequest,
    update::{
        machine::Unhandled,
        overlay::{OverlayContentMessage, OverlayMessage},
    },
};
use rstest::rstest;

use crate::support::table::cell;

fn digits(input: &str, error: Option<TimecodeError>) -> Option<Overlay> {
    Some(Overlay::JumpToTime(TextEntry {
        input: input.to_string(),
        error,
    }))
}

fn typed(text_request: TextRequest) -> OverlayMessage {
    OverlayMessage::Inner(OverlayContentMessage::Text(text_request))
}

#[rstest]
#[case::empty_digit_starts_the_time(
    digits("", None),
    TextRequest::Char('1'),
    Ok((digits("1", None), Cmd::none()))
)]
#[case::digit_appends(
    digits("1", None),
    TextRequest::Char('0'),
    Ok((digits("10", None), Cmd::none()))
)]
#[case::colon_appends(
    digits("1", None),
    TextRequest::Char(':'),
    Ok((digits("1:", None), Cmd::none()))
)]
#[case::digit_clears_a_stale_error(
    digits("5:", Some(TimecodeError::Malformed)),
    TextRequest::Char('3'),
    Ok((digits("5:3", None), Cmd::none()))
)]
#[case::letter_is_refused(digits("1", None), TextRequest::Char('a'), Err(Unhandled))]
#[case::space_is_refused(digits("1", None), TextRequest::Char(' '), Err(Unhandled))]
#[case::full_input_refuses_a_ninth_char(
    digits("1:02:034", None),
    TextRequest::Char('5'),
    Err(Unhandled)
)]
#[case::backspace_erases_and_clears_the_error(
    digits("5:", Some(TimecodeError::Malformed)),
    TextRequest::Backspace,
    Ok((digits("5", None), Cmd::none()))
)]
#[case::backspace_on_empty_is_refused(
    digits("", None),
    TextRequest::Backspace,
    Err(Unhandled)
)]
fn jump_cell(
    #[case] overlay: Option<Overlay>,
    #[case] text_request: TextRequest,
    #[case] expected: Result<(Option<Overlay>, Cmd), Unhandled>,
) {
    cell(overlay, typed(text_request), expected);
}
