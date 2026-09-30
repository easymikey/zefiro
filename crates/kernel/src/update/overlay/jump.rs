use crate::{
    domain::{JumpDigits, JumpInputLimits},
    message::TextRequest,
    update::{
        machine::{Machine, Rejected},
        overlay::OverlayEffect,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpError {
    NotTimecodeChar,
    Full,
}

impl Machine for JumpDigits {
    type Message = TextRequest;
    type Error = JumpError;
    type Effect = OverlayEffect;

    fn transition(
        mut self,
        message: TextRequest,
    ) -> Result<(Self, OverlayEffect), Rejected<Self>> {
        match message {
            TextRequest::Char(character)
                if !(character.is_ascii_digit()
                    || character == JumpDigits::SEPARATOR) =>
            {
                Err(Rejected {
                    state: self,
                    reason: JumpError::NotTimecodeChar,
                })
            }
            TextRequest::Char(_)
                if self.input.len() >= JumpInputLimits::default().max_len =>
            {
                Err(Rejected {
                    state: self,
                    reason: JumpError::Full,
                })
            }
            TextRequest::Char(character) => {
                self.input.push(character);
                self.error = None;
                Ok((self, OverlayEffect::default()))
            }
            TextRequest::Backspace => {
                self.input.pop();
                self.error = None;
                Ok((self, OverlayEffect::default()))
            }
        }
    }
}
