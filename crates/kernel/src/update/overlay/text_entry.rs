use crate::{
    cmd::Cmd,
    domain::overlay::TextEntry,
    message::TextRequest,
    update::machine::{Machine, Unhandled},
};

impl<E> Machine for TextEntry<E> {
    type Message = TextRequest;
    type Effect = Cmd;

    fn transition(&mut self, text_request: TextRequest) -> Result<Cmd, Unhandled> {
        match text_request {
            TextRequest::Char(character) => self.input.push(character),
            TextRequest::Backspace => {
                self.input.pop().ok_or(Unhandled)?;
            }
        }
        self.error = None;
        Ok(Cmd::none())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::overlay::{MusicDirError, TextEntry},
        message::TextRequest,
        update::machine::{Machine, Unhandled},
    };

    fn entry(input: &str, error: Option<MusicDirError>) -> TextEntry<MusicDirError> {
        TextEntry {
            input: input.to_string(),
            error,
        }
    }

    #[rstest]
    #[case::char_starts_the_text(
        entry("", None),
        TextRequest::Char('m'),
        Ok(entry("m", None))
    )]
    #[case::char_appends(
        entry("mi", None),
        TextRequest::Char('x'),
        Ok(entry("mix", None))
    )]
    #[case::space_is_a_char(
        entry("my", None),
        TextRequest::Char(' '),
        Ok(entry("my ", None))
    )]
    #[case::char_clears_the_error(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::Char('m'),
        Ok(entry("m", None))
    )]
    #[case::backspace_erases_the_last_char(
        entry("mix", None),
        TextRequest::Backspace,
        Ok(entry("mi", None))
    )]
    #[case::backspace_clears_the_error(
        entry("m", Some(MusicDirError::Empty)),
        TextRequest::Backspace,
        Ok(entry("", None))
    )]
    #[case::backspace_on_empty_is_unhandled(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::Backspace,
        Err(Unhandled)
    )]
    fn text_entry_cell(
        #[case] mut text_entry: TextEntry<MusicDirError>,
        #[case] text_request: TextRequest,
        #[case] expected: Result<TextEntry<MusicDirError>, Unhandled>,
    ) {
        let outcome = text_entry
            .transition(text_request)
            .map(|_| text_entry.clone());
        assert_eq!(outcome, expected);
    }
}
