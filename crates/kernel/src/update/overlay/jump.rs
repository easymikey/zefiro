use crate::{
    Cmd,
    domain::JumpDigits,
    message::TextRequest,
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDigitsMessage {
    Char(char),
    Backspace,
}

impl From<TextRequest> for JumpDigitsMessage {
    fn from(request: TextRequest) -> Self {
        match request {
            TextRequest::Char(character) => JumpDigitsMessage::Char(character),
            TextRequest::Backspace => JumpDigitsMessage::Backspace,
        }
    }
}

impl Machine for JumpDigits {
    type Message = JumpDigitsMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: JumpDigitsMessage) -> Result<Cmd, Unhandled> {
        match message {
            JumpDigitsMessage::Char(character)
                if !(character.is_ascii_digit()
                    || character == JumpDigits::SEPARATOR) =>
            {
                Err(Unhandled)
            }
            JumpDigitsMessage::Char(_) if self.input.len() >= JumpDigits::MAX_LEN => {
                Err(Unhandled)
            }
            JumpDigitsMessage::Char(character) => {
                self.input.push(character);
                self.error = None;
                Ok(Cmd::none())
            }
            JumpDigitsMessage::Backspace => {
                self.input.pop();
                self.error = None;
                Ok(Cmd::none())
            }
        }
    }
}
