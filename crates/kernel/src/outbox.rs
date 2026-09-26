use crate::message::Message;

pub trait Outbox<F: Into<Message>> {
    fn send(&self, fact: F) -> Delivery;
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    Congested,
    Closed,
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crate::{
        message::{LibraryFact, Message},
        outbox::{Delivery, Outbox},
    };

    struct FakeOutbox {
        sent: RefCell<Vec<Message>>,
    }

    impl Outbox<LibraryFact> for FakeOutbox {
        fn send(&self, fact: LibraryFact) -> Delivery {
            self.sent.borrow_mut().push(fact.into());
            Delivery::Sent
        }
    }

    #[test]
    fn a_fake_outbox_delivers_facts_as_messages() {
        let outbox = FakeOutbox {
            sent: RefCell::new(Vec::new()),
        };

        let delivery = outbox.send(LibraryFact::HistoryLoaded(Vec::new()));

        assert_eq!(delivery, Delivery::Sent);
        assert_eq!(
            outbox.sent.borrow().as_slice(),
            [Message::Library(LibraryFact::HistoryLoaded(Vec::new()))]
        );
    }
}
