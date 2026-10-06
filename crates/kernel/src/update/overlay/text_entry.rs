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
        match message {
            TextRequest::Char(character) => {
                if !E::accepts(character) || self.input.len() >= E::MAX_LEN {
                    return Err(Unhandled);
                }
                self.input.push(character);
            }
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

    #[test]
    fn backspace_on_empty_is_unhandled_and_leaves_the_state() {
        let mut text_entry = entry("", Some(MusicDirError::Empty));
        let outcome = text_entry.transition(TextRequest::Backspace);
        assert_eq!(outcome, Err(Unhandled));
        assert_eq!(text_entry, entry("", Some(MusicDirError::Empty)));
    }

    fn jump_entry(input: &str) -> TextEntry<TimecodeError> {
        TextEntry {
            input: input.to_string(),
            error: Some(TimecodeError::Malformed),
        }
    }

    #[rstest]
    #[case::a_letter("5", TextRequest::Char('a'))]
    #[case::a_space("5", TextRequest::Char(' '))]
    #[case::a_ninth_char("12:34:56", TextRequest::Char('7'))]
    #[case::the_separator_past_the_cap("12:34:56", TextRequest::Char(':'))]
    #[case::backspace_on_empty("", TextRequest::Backspace)]
    fn jump_text_entry_refuses_and_keeps_the_state(
        #[case] input: &str,
        #[case] text_request: TextRequest,
    ) {
        let mut text_entry = jump_entry(input);
        let outcome = text_entry.transition(text_request);
        assert_eq!(outcome, Err(Unhandled));
        assert_eq!(text_entry, jump_entry(input));
    }

    #[rstest]
    #[case::a_digit("5", TextRequest::Char('3'), "53")]
    #[case::the_separator("5", TextRequest::Char(':'), "5:")]
    #[case::the_eighth_char("12:34:5", TextRequest::Char('6'), "12:34:56")]
    #[case::backspace("53", TextRequest::Backspace, "5")]
    fn jump_text_entry_takes_what_it_admits_and_clears_the_error(
        #[case] input: &str,
        #[case] text_request: TextRequest,
        #[case] expected: &str,
    ) {
        let mut text_entry = jump_entry(input);
        let outcome = text_entry
            .transition(text_request)
            .map(|_| text_entry.clone());
        assert_eq!(
            outcome,
            Ok(TextEntry {
                input: expected.to_string(),
                error: None,
            })
        );
    }
}
