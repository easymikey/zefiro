use crate::{
    domain::{JumpDigits, JumpInputLimits},
    message::JumpRequest,
    update::{
        machine::{Machine, Rejected},
        overlay::OverlayEffect,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpRejection {
    NotTimecodeChar,
    Full,
}

impl Machine for JumpDigits {
    type Message = JumpRequest;
    type Rejection = JumpRejection;
    type Effect = OverlayEffect;

    fn transition(
        mut self,
        message: JumpRequest,
    ) -> Result<(Self, OverlayEffect), Rejected<Self>> {
        match message {
            JumpRequest::Char(character)
                if !(character.is_ascii_digit()
                    || character == JumpDigits::SEPARATOR) =>
            {
                Err(Rejected {
                    state: self,
                    reason: JumpRejection::NotTimecodeChar,
                })
            }
            JumpRequest::Char(_)
                if self.input.len() >= JumpInputLimits::default().max_len =>
            {
                Err(Rejected {
                    state: self,
                    reason: JumpRejection::Full,
                })
            }
            JumpRequest::Char(character) => {
                self.input.push(character);
                self.error = None;
                Ok((self, OverlayEffect::default()))
            }
            JumpRequest::Backspace => {
                self.input.pop();
                self.error = None;
                Ok((self, OverlayEffect::default()))
            }
        }
    }
}
