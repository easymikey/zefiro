use crate::{domain::overlay::TextEntry, message::TextRequest};

pub(crate) fn retype(text: &mut TextEntry, message: TextRequest) {
    match message {
        TextRequest::Char(character) => text.input.push(character),
        TextRequest::Backspace => {
            text.input.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::overlay::TextEntry,
        message::TextRequest,
        update::overlay::text::retype,
    };

    fn entry(input: &str) -> TextEntry {
        TextEntry {
            input: input.to_string(),
        }
    }

    #[rstest]
    #[case::empty_char_starts_the_text(entry(""), TextRequest::Char('m'), entry("m"))]
    #[case::char_appends(entry("mi"), TextRequest::Char('x'), entry("mix"))]
    #[case::space_is_a_char(entry("my"), TextRequest::Char(' '), entry("my "))]
    #[case::backspace_erases_the_last_char(
        entry("mix"),
        TextRequest::Backspace,
        entry("mi")
    )]
    #[case::backspace_on_empty_stays_empty(
        entry(""),
        TextRequest::Backspace,
        entry("")
    )]
    fn text_entry_cell(
        #[case] mut start: TextEntry,
        #[case] message: TextRequest,
        #[case] next: TextEntry,
    ) {
        retype(&mut start, message);
        assert_eq!(start, next);
    }
}
