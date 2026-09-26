use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{Delivery, Message, Outbox};

#[derive(Debug, Clone, Default)]
pub(crate) struct Congestion(Arc<AtomicBool>);

impl Congestion {
    pub(crate) fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn settle(&self) -> Crowding {
        if self.0.swap(false, Ordering::AcqRel) {
            Crowding::Crowded
        } else {
            Crowding::Clear
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Crowding {
    Crowded,
    Clear,
}

#[derive(Debug)]
pub struct Mailbox<F> {
    sender: Sender<Message>,
    congestion: Congestion,
    fact: PhantomData<fn(F)>,
}

impl<F> Mailbox<F> {
    pub(crate) fn new(sender: Sender<Message>, congestion: Congestion) -> Self {
        Self {
            sender,
            congestion,
            fact: PhantomData,
        }
    }
}

impl<F: Into<Message>> Outbox<F> for Mailbox<F> {
    fn send(&self, fact: F) -> Delivery {
        match self.sender.try_send(fact.into()) {
            Ok(()) => Delivery::Sent,
            Err(TrySendError::Full(message)) => {
                self.congestion.raise();
                match self.sender.send(message) {
                    Ok(()) => Delivery::Congested,
                    Err(_) => Delivery::Closed,
                }
            }
            Err(TrySendError::Disconnected(_)) => Delivery::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::bounded;
    use kernel::{AudioEvent, Delivery, Outbox};
    use rstest::rstest;

    use crate::mailbox::{Congestion, Crowding, Mailbox};

    #[derive(Clone, Copy)]
    enum Scenario {
        Room,
        ReceiverDropped,
        Full,
    }

    #[rstest]
    #[case::room(Scenario::Room, Delivery::Sent, Crowding::Clear)]
    #[case::receiver_dropped(
        Scenario::ReceiverDropped,
        Delivery::Closed,
        Crowding::Clear
    )]
    #[case::full(Scenario::Full, Delivery::Congested, Crowding::Crowded)]
    fn a_mailbox_reports_its_delivery(
        #[case] scenario: Scenario,
        #[case] expected: Delivery,
        #[case] crowding: Crowding,
    ) {
        let (sender, receiver) = bounded(1);
        let congestion = Congestion::default();
        let mailbox = Mailbox::new(sender, congestion.clone());
        let drainer = match scenario {
            Scenario::Room => None,
            Scenario::ReceiverDropped => {
                drop(receiver);
                None
            }
            Scenario::Full => {
                assert_eq!(mailbox.send(AudioEvent::TrackChanged), Delivery::Sent);
                Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    receiver.recv().unwrap();
                    receiver.recv().unwrap();
                }))
            }
        };

        let delivery = mailbox.send(AudioEvent::TrackChanged);

        assert_eq!(delivery, expected);
        assert_eq!(congestion.settle(), crowding);
        if let Some(drainer) = drainer {
            drainer.join().unwrap();
        }
    }
}
