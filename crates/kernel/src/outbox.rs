use thiserror::Error;

use crate::message::Message;

pub trait Outbox<F: Into<Message>> {
    fn send(&self, event: F) -> Result<(), SendError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SendError {
    #[error("the mailbox was full")]
    Full,
    #[error("the mailbox is closed")]
    Closed,
}

pub trait Refusals {
    fn refused(&self, input: &'static str) -> Result<(), SendError>;
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crate::{
        message::{LibraryEvent, Message},
        outbox::{Outbox, SendError},
    };

    struct FakeOutbox {
        sent: RefCell<Vec<Message>>,
    }

    impl Outbox<LibraryEvent> for FakeOutbox {
        fn send(&self, event: LibraryEvent) -> Result<(), SendError> {
            self.sent.borrow_mut().push(event.into());
            Ok(())
        }
    }

    #[test]
    fn a_fake_outbox_sends_events_as_messages() {
        let outbox = FakeOutbox {
            sent: RefCell::new(Vec::new()),
        };

        let delivery = outbox.send(LibraryEvent::HistoryLoaded(Vec::new()));

        assert_eq!(delivery, Ok(()));
        assert_eq!(
            outbox.sent.borrow().as_slice(),
            [Message::Library(LibraryEvent::HistoryLoaded(Vec::new()))]
        );
    }
}
