use crate::{
    cmd::Cmd,
    domain::overlay::{Accepts, TextEntry},
    message::TextRequest,
    update::machine::{Machine, Unhandled},
};

impl<E: Accepts> Machine for TextEntry<E> {
    type Message = TextRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: TextRequest) -> Result<Cmd, Unhandled> {
        if let TextRequest::Char(character) = message
            && (!E::accepts(character) || self.input.len() >= E::MAX_LEN)
        {
            return Err(Unhandled);
        }
        edit(&mut self.input, message)?;
        self.error = None;
        Ok(Cmd::none())
    }
}

pub(crate) fn edit(
    input: &mut String,
    text_request: TextRequest,
) -> Result<(), Unhandled> {
    match text_request {
        TextRequest::Backspace | TextRequest::DeleteWord | TextRequest::Clear
            if input.is_empty() =>
        {
            return Err(Unhandled);
        }
        TextRequest::Char(character) => input.push(character),
        TextRequest::Backspace => {
            input.pop();
        }
        TextRequest::DeleteWord => delete_trailing_word(input),
        TextRequest::Clear => input.clear(),
    }
    Ok(())
}

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        domain::{
            overlay::{MusicDirError, TextEntry},
            time::TimecodeError,
        },
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
        (Ok(()), entry("m", None))
    )]
    #[case::char_appends(
        entry("mi", None),
        TextRequest::Char('x'),
        (Ok(()), entry("mix", None))
    )]
    #[case::space_is_a_char(
        entry("my", None),
        TextRequest::Char(' '),
        (Ok(()), entry("my ", None))
    )]
    #[case::char_clears_the_error(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::Char('m'),
        (Ok(()), entry("m", None))
    )]
    #[case::backspace_erases_the_last_char(
        entry("mix", None),
        TextRequest::Backspace,
        (Ok(()), entry("mi", None))
    )]
    #[case::backspace_clears_the_error(
        entry("m", Some(MusicDirError::Empty)),
        TextRequest::Backspace,
        (Ok(()), entry("", None))
    )]
    #[case::backspace_on_empty_is_unhandled_and_leaves_the_state(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::Backspace,
        (Err(Unhandled), entry("", Some(MusicDirError::Empty)))
    )]
    #[case::delete_word_keeps_the_text_before_the_last_word(
        entry("my songs", Some(MusicDirError::Empty)),
        TextRequest::DeleteWord,
        (Ok(()), entry("my ", None))
    )]
    #[case::delete_word_takes_the_trailing_spaces_with_the_word(
        entry("my songs  ", None),
        TextRequest::DeleteWord,
        (Ok(()), entry("my ", None))
    )]
    #[case::clear_empties_the_text_and_the_error(
        entry("my songs", Some(MusicDirError::Empty)),
        TextRequest::Clear,
        (Ok(()), entry("", None))
    )]
    #[case::delete_word_on_empty_is_unhandled_and_leaves_the_state(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::DeleteWord,
        (Err(Unhandled), entry("", Some(MusicDirError::Empty)))
    )]
    #[case::clear_on_empty_is_unhandled_and_leaves_the_state(
        entry("", Some(MusicDirError::Empty)),
        TextRequest::Clear,
        (Err(Unhandled), entry("", Some(MusicDirError::Empty)))
    )]
    fn text_entry_cell(
        #[case] mut text_entry: TextEntry<MusicDirError>,
        #[case] text_request: TextRequest,
        #[case] expected: (Result<(), Unhandled>, TextEntry<MusicDirError>),
    ) {
        let outcome = text_entry.transition(text_request).map(drop);
        assert_eq!((outcome, text_entry), expected);
    }

    fn jump_entry(input: &str) -> TextEntry<TimecodeError> {
        TextEntry {
            input: input.to_string(),
            error: Some(TimecodeError::Malformed),
        }
    }

    fn admitted(input: &str) -> (Result<(), Unhandled>, TextEntry<TimecodeError>) {
        (
            Ok(()),
            TextEntry {
                input: input.to_string(),
                error: None,
            },
        )
    }

    fn refused(input: &str) -> (Result<(), Unhandled>, TextEntry<TimecodeError>) {
        (Err(Unhandled), jump_entry(input))
    }

    #[rstest]
    #[case::a_letter("5", TextRequest::Char('a'), refused("5"))]
    #[case::a_space("5", TextRequest::Char(' '), refused("5"))]
    #[case::a_ninth_char("12:34:56", TextRequest::Char('7'), refused("12:34:56"))]
    #[case::the_separator_past_the_cap(
        "12:34:56",
        TextRequest::Char(':'),
        refused("12:34:56")
    )]
    #[case::backspace_on_empty("", TextRequest::Backspace, refused(""))]
    #[case::a_digit("5", TextRequest::Char('3'), admitted("53"))]
    #[case::the_separator("5", TextRequest::Char(':'), admitted("5:"))]
    #[case::the_eighth_char("12:34:5", TextRequest::Char('6'), admitted("12:34:56"))]
    #[case::backspace("53", TextRequest::Backspace, admitted("5"))]
    #[case::clear("12:34", TextRequest::Clear, admitted(""))]
    #[case::delete_word("12:34", TextRequest::DeleteWord, admitted(""))]
    fn jump_text_entry_takes_what_it_admits_and_clears_the_error(
        #[case] input: &str,
        #[case] text_request: TextRequest,
        #[case] expected: (Result<(), Unhandled>, TextEntry<TimecodeError>),
    ) {
        let mut text_entry = jump_entry(input);
        let outcome = text_entry.transition(text_request).map(drop);
        assert_eq!((outcome, text_entry), expected);
    }
}
