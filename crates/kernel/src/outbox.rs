use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Sender, TrySendError};
use thiserror::Error;

use crate::{
    domain::Driver,
    message::{AudioEvent, DriverEvent, Message},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SendError {
    #[error("the mailbox was full")]
    Full,
    #[error("the mailbox is closed")]
    Closed,
}

#[derive(Debug, Clone, Default)]
pub struct Congestion(Arc<AtomicBool>);

impl Congestion {
    pub fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

#[derive(Debug)]
pub struct Outbox<F> {
    sender: Sender<Message>,
    full_edge: Congestion,
    event: PhantomData<fn(F)>,
}

impl<F> Outbox<F> {
    #[must_use]
    pub fn new(sender: Sender<Message>, full_edge: Congestion) -> Self {
        Self {
            sender,
            full_edge,
            event: PhantomData,
        }
    }

    pub fn report(
        &self,
        driver: Driver,
        message: DriverEvent,
    ) -> Result<(), SendError> {
        self.send_message(Message::Driver {
            driver,
            event: message,
        })
    }

    fn send_message(&self, message: Message) -> Result<(), SendError> {
        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(message)) => {
                self.full_edge.raise();
                match self.sender.send(message) {
                    Ok(()) => Err(SendError::Full),
                    Err(_) => Err(SendError::Closed),
                }
            }
            Err(TrySendError::Disconnected(_)) => Err(SendError::Closed),
        }
    }
}

impl<F: Into<Message>> Outbox<F> {
    pub fn send(&self, event: F) -> Result<(), SendError> {
        self.send_message(event.into())
    }
}

impl Outbox<AudioEvent> {
    pub fn refused(&self, input: &'static str) -> Result<(), SendError> {
        self.report(Driver::Audio, DriverEvent::Rejected { input })
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::bounded;
    use rstest::rstest;

    use crate::{
        message::AudioEvent,
        outbox::{Congestion, Outbox, SendError},
    };

    #[derive(Clone, Copy)]
    enum Scenario {
        Room,
        ReceiverDropped,
        Full,
    }

    #[rstest]
    #[case::room(Scenario::Room, Ok(()), false)]
    #[case::receiver_dropped(Scenario::ReceiverDropped, Err(SendError::Closed), false)]
    #[case::full(Scenario::Full, Err(SendError::Full), true)]
    fn a_sender_reports_the_outcome_of_a_send(
        #[case] scenario: Scenario,
        #[case] expected: Result<(), SendError>,
        #[case] raised: bool,
    ) {
        let (sender, receiver) = bounded(1);
        let full_edge = Congestion::default();
        let sender_under_test = Outbox::new(sender, full_edge.clone());
        let drainer = match scenario {
            Scenario::Room => None,
            Scenario::ReceiverDropped => {
                drop(receiver);
                None
            }
            Scenario::Full => {
                assert_eq!(sender_under_test.send(AudioEvent::TrackChanged), Ok(()));
                Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    receiver.recv().unwrap();
                    receiver.recv().unwrap();
                }))
            }
        };

        let delivery = sender_under_test.send(AudioEvent::TrackChanged);

        assert_eq!(delivery, expected);
        assert_eq!(full_edge.take(), raised);
        if let Some(drainer) = drainer {
            drainer.join().unwrap();
        }
    }
}
