use std::collections::VecDeque;

use kernel::domain::driver::DriverName;
use strum::IntoStaticStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
pub(crate) enum DropReason {
    NotRunning,
    Full,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TraceEntry {
    Dropped {
        driver: DriverName,
        command: &'static str,
        reason: DropReason,
    },
    ControlsUnattached,
    Error(TraceError),
    TimerOverflow(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TraceError {
    Join(DriverName),
    Restart(DriverName),
}

#[derive(Debug, Default)]
pub(crate) struct Trace {
    entries: VecDeque<TraceEntry>,
}

impl Trace {
    pub(crate) const CAPACITY: usize = 128;

    pub(crate) fn push(&mut self, traced: TraceEntry) {
        if self.entries.len() == Self::CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(traced);
    }

    pub(crate) fn record(&mut self, result: Result<(), TraceEntry>) {
        match result {
            Err(entry) if self.entries.back() != Some(&entry) => self.push(entry),
            Ok(()) | Err(_) => {}
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
    use kernel::domain::driver::DriverName;

    use crate::trace::{DropReason, Trace, TraceEntry};

    fn dropped(driver: DriverName) -> TraceEntry {
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

        trace.push(dropped(DriverName::Audio));
        trace.push(dropped(DriverName::Config));

        let entries: Vec<_> = trace.iter().collect();
        assert_eq!(
            entries,
            vec![&dropped(DriverName::Audio), &dropped(DriverName::Config)]
        );
    }

    #[test]
    fn the_ring_drops_the_oldest_entry_past_capacity() {
        let mut trace = Trace::default();

        for _ in 0..Trace::CAPACITY {
            trace.push(dropped(DriverName::Audio));
        }
        trace.push(dropped(DriverName::Macos));

        assert_eq!(trace.len(), Trace::CAPACITY);
        assert_eq!(trace.iter().next(), Some(&dropped(DriverName::Audio)));
        assert_eq!(trace.iter().last(), Some(&dropped(DriverName::Macos)));
    }

    #[test]
    fn a_controls_unattached_entry_is_kept_distinct_from_a_dropped_entry() {
        let mut trace = Trace::default();

        trace.push(TraceEntry::ControlsUnattached);
        trace.push(dropped(DriverName::Macos));

        let entries: Vec<_> = trace.iter().collect();
        assert_eq!(
            entries,
            vec![&TraceEntry::ControlsUnattached, &dropped(DriverName::Macos)]
        );
    }
}
