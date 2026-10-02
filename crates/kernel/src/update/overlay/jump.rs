use crate::{
    domain::JumpDigits,
    message::TextRequest,
    update::{machine::Machine, overlay::OverlayOutcome},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JumpError {
    #[error("not a timecode character")]
    NotTimecodeChar,
    #[error("timecode is full")]
    Full,
}

impl Machine for JumpDigits {
    type Message = TextRequest;
    type Error = JumpError;
    type Effect = OverlayOutcome;

    fn transition(
        &mut self,
        message: TextRequest,
    ) -> Result<OverlayOutcome, JumpError> {
        match message {
            TextRequest::Char(character)
                if !(character.is_ascii_digit()
                    || character == JumpDigits::SEPARATOR) =>
            {
                Err(JumpError::NotTimecodeChar)
            }
            TextRequest::Char(_) if self.input.len() >= JumpDigits::MAX_LEN => {
                Err(JumpError::Full)
            }
            TextRequest::Char(character) => {
                self.input.push(character);
                self.error = None;
                Ok(OverlayOutcome::default())
            }
            TextRequest::Backspace => {
                self.input.pop();
                self.error = None;
                Ok(OverlayOutcome::default())
            }
        }
    }
}
