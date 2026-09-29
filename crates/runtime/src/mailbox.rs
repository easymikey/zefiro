use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{Delivery, DriverMessage, Message, Outbox, domain::Driver};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Episode {
    #[default]
    Quiet,
    Congested,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Observation {
    pub(crate) crowding: Crowding,
    pub(crate) backlog: Backlog,
    pub(crate) driver: Driver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backlog {
    Drained,
    Pending,
}

pub(crate) fn episode_transition(
    episode: Episode,
    observed: Observation,
) -> (Episode, Option<Message>) {
    match (episode, observed.crowding, observed.backlog) {
        (Episode::Quiet, Crowding::Crowded, _) => (
            Episode::Congested,
            Some(Message::Driver(observed.driver, DriverMessage::Congested)),
        ),
        (Episode::Congested, Crowding::Clear, Backlog::Drained) => {
            (Episode::Quiet, None)
        }
        (episode, _, _) => (episode, None),
    }
}

#[derive(Debug)]
pub(crate) struct Mailbox<F> {
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

    pub(crate) fn report(&self, driver: Driver, message: DriverMessage) -> Delivery {
        self.deliver(Message::Driver(driver, message))
    }

    fn deliver(&self, message: Message) -> Delivery {
        match self.sender.try_send(message) {
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

impl<F: Into<Message>> Outbox<F> for Mailbox<F> {
    fn send(&self, fact: F) -> Delivery {
        self.deliver(fact.into())
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::bounded;
    use kernel::{
        AudioEvent,
        Delivery,
        DriverMessage,
        Message,
        Outbox,
        domain::Driver,
    };
    use rstest::rstest;

    use crate::mailbox::{
        Backlog,
        Congestion,
        Crowding,
        Episode,
        Mailbox,
        Observation,
        episode_transition,
    };

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

    #[rstest]
    #[case::quiet_clear_drained(
        (Episode::Quiet, Crowding::Clear, Backlog::Drained),
        Episode::Quiet,
        false
    )]
    #[case::quiet_clear_pending(
        (Episode::Quiet, Crowding::Clear, Backlog::Pending),
        Episode::Quiet,
        false
    )]
    #[case::quiet_crowded_drained(
        (Episode::Quiet, Crowding::Crowded, Backlog::Drained),
        Episode::Congested,
        true
    )]
    #[case::quiet_crowded_pending(
        (Episode::Quiet, Crowding::Crowded, Backlog::Pending),
        Episode::Congested,
        true
    )]
    #[case::congested_clear_drained(
        (Episode::Congested, Crowding::Clear, Backlog::Drained),
        Episode::Quiet,
        false
    )]
    #[case::congested_clear_pending(
        (Episode::Congested, Crowding::Clear, Backlog::Pending),
        Episode::Congested,
        false
    )]
    #[case::congested_crowded_drained(
        (Episode::Congested, Crowding::Crowded, Backlog::Drained),
        Episode::Congested,
        false
    )]
    #[case::congested_crowded_pending(
        (Episode::Congested, Crowding::Crowded, Backlog::Pending),
        Episode::Congested,
        false
    )]
    fn an_episode_opens_once_and_closes_when_drained(
        #[case] given: (Episode, Crowding, Backlog),
        #[case] expected: Episode,
        #[case] emits: bool,
    ) {
        let (episode, crowding, backlog) = given;
        let observed = Observation {
            crowding,
            backlog,
            driver: Driver::Library,
        };

        let (next, message) = episode_transition(episode, observed);

        assert_eq!(next, expected);
        assert_eq!(
            message,
            emits.then_some(Message::Driver(Driver::Library, DriverMessage::Congested))
        );
    }
}
