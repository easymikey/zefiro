use std::collections::VecDeque;

use kernel::{domain::Driver, update::UpdateError};
use strum::IntoStaticStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
pub(crate) enum DropReason {
    NotRunning,
    Full,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TraceEntry {
    Rejected {
        message: &'static str,
        error: UpdateError,
    },
    Dropped {
        driver: Driver,
        command: &'static str,
        reason: DropReason,
    },
    ControlsUnattached,
    JoinFailed {
        driver: Driver,
    },
    RestartFailed {
        driver: Driver,
    },
    TimerOverflow {
        timer: &'static str,
    },
}

#[derive(Debug, Default)]
pub(crate) struct Trace {
    entries: VecDeque<TraceEntry>,
}

impl Trace {
    pub(crate) const CAPACITY: usize = 128;

    pub(crate) fn push(&mut self, entry: TraceEntry) {
        if self.entries.len() == Self::CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub(crate) fn record(&mut self, result: Result<(), TraceEntry>) {
        if let Err(entry) = result {
            self.push(entry);
        }
    }

    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &TraceEntry> {
        self.entries.iter()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::Driver,
        update::{DriverStatusError, UpdateError},
    };

    use crate::trace::{DropReason, Trace, TraceEntry};

    fn dropped(driver: Driver) -> TraceEntry {
        TraceEntry::Dropped {
            driver,
            command: "save",
            reason: DropReason::NotRunning,
        }
    }

    #[test]
    fn a_fresh_trace_is_empty() {
        let trace = Trace::default();

        assert!(trace.is_empty());
        assert_eq!(trace.len(), 0);
    }

    #[test]
    fn a_pushed_entry_is_kept_in_order() {
        let mut trace = Trace::default();

        trace.push(dropped(Driver::Audio));
        trace.push(dropped(Driver::Config));

        let entries: Vec<_> = trace.iter().collect();
        assert_eq!(
            entries,
            vec![&dropped(Driver::Audio), &dropped(Driver::Config)]
        );
    }

    #[test]
    fn the_ring_drops_the_oldest_entry_past_capacity() {
        let mut trace = Trace::default();

        for _ in 0..Trace::CAPACITY {
            trace.push(dropped(Driver::Audio));
        }
        trace.push(dropped(Driver::Macos));

        assert_eq!(trace.len(), Trace::CAPACITY);
        assert_eq!(trace.iter().next(), Some(&dropped(Driver::Audio)));
        assert_eq!(trace.iter().last(), Some(&dropped(Driver::Macos)));
    }

    #[test]
    fn a_controls_unattached_entry_is_kept_distinct_from_a_dropped_entry() {
        let mut trace = Trace::default();

        trace.push(TraceEntry::ControlsUnattached);
        trace.push(dropped(Driver::Macos));

        let entries: Vec<_> = trace.iter().collect();
        assert_eq!(
            entries,
            vec![&TraceEntry::ControlsUnattached, &dropped(Driver::Macos)]
        );
    }

    #[test]
    fn a_rejected_entry_carries_the_message_label_and_the_rejection() {
        let mut trace = Trace::default();
        let error = UpdateError::Driver(Driver::Library, DriverStatusError::Dead);

        trace.push(TraceEntry::Rejected {
            message: "elapsed",
            error: error.clone(),
        });

        assert_eq!(
            trace.iter().next(),
            Some(&TraceEntry::Rejected {
                message: "elapsed",
                error,
            })
        );
    }
}
