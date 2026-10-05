use crate::{
    domain::{
        overlay::{JumpDigits, TextEntry},
        time::TimecodeError,
    },
    message::TextRequest,
};

#[must_use]
pub(crate) fn admits(
    text_entry: &TextEntry<TimecodeError>,
    text_request: TextRequest,
) -> bool {
    match text_request {
        TextRequest::Char(character) => {
            (character.is_ascii_digit() || character == JumpDigits::SEPARATOR)
                && text_entry.input.len() < JumpDigits::MAX_LEN
        }
        TextRequest::Backspace => true,
    }
}
