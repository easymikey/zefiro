use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::message::Message;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum SendError {
    #[error("the inbox is closed")]
    Closed,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Congestion(Arc<AtomicBool>);

impl Congestion {
    pub(crate) fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub(crate) fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

#[derive(Debug)]
pub(crate) struct Outbox<F> {
    inbox: Sender<Message>,
    full: Congestion,
    event: PhantomData<fn(F)>,
}

impl<F> Outbox<F> {
    #[must_use]
    pub(crate) fn new(inbox: Sender<Message>, full: Congestion) -> Self {
        Self {
            inbox,
            full,
            event: PhantomData,
        }
    }

    fn send_message(&self, message: Message) -> Result<(), SendError> {
        match self.inbox.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(message)) => {
                self.full.raise();
                match self.inbox.send(message) {
                    Ok(()) => Ok(()),
                    Err(_) => Err(SendError::Closed),
                }
            }
            Err(TrySendError::Disconnected(_)) => Err(SendError::Closed),
        }
    }
}

impl<F: Into<Message>> Outbox<F> {
    pub(crate) fn send(&self, event: F) -> Result<(), SendError> {
        self.send_message(event.into())
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::bounded;
    use kernel::message::AudioEvent;
    use rstest::rstest;

    use crate::outbox::{Congestion, Outbox, SendError};

    #[derive(Clone, Copy)]
    enum Scenario {
        Room,
        ReceiverDropped,
        Full,
    }

    #[rstest]
    #[case::room(Scenario::Room, Ok(()))]
    #[case::receiver_dropped(Scenario::ReceiverDropped, Err(SendError::Closed))]
    #[case::full(Scenario::Full, Ok(()))]
    fn a_sender_reports_the_outcome_of_a_send(
        #[case] scenario: Scenario,
        #[case] expected: Result<(), SendError>,
    ) {
        let (inbox, receiver) = bounded(1);
        let full = Congestion::default();
        let sender_under_test = Outbox::new(inbox, full.clone());
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
        assert_eq!(full.take(), matches!(scenario, Scenario::Full));
        if let Some(drainer) = drainer {
            drainer.join().unwrap();
        }
    }
}
