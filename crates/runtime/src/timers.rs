use std::{
    mem,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Scheduled<M> {
    deadline_at: Instant,
    message: M,
}

#[derive(Debug)]
pub(crate) struct Timers<M> {
    scheduled: Vec<Scheduled<M>>,
}

impl<M> Default for Timers<M> {
    fn default() -> Self {
        Self {
            scheduled: Vec::new(),
        }
    }
}

impl<M> Timers<M> {
    pub(crate) fn schedule(&mut self, deadline_at: Instant, message: M) {
        self.scheduled.retain(|held| {
            mem::discriminant(&held.message) != mem::discriminant(&message)
        });
        self.scheduled.push(Scheduled {
            deadline_at,
            message,
        });
    }

    pub(crate) fn after(&mut self, delay: Duration, message: M) {
        if let Some(deadline_at) = Instant::now().checked_add(delay) {
            self.schedule(deadline_at, message);
        }
    }

    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.scheduled
            .iter()
            .map(|scheduled| scheduled.deadline_at)
            .min()
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<M> {
        self.scheduled
            .sort_by_key(|scheduled| scheduled.deadline_at);
        let due = self
            .scheduled
            .partition_point(|scheduled| scheduled.deadline_at <= now);
        self.scheduled
            .drain(..due)
            .map(|scheduled| scheduled.message)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::{domain::revision::Revision, message::Timer};
    use rstest::rstest;

    use crate::timers::Timers;

    fn revision_after(steps: u64) -> Revision {
        (0..steps).fold(Revision::default(), |revision, _| revision.next())
    }

    fn toast(revision: Revision) -> Timer {
        Timer::Toast(revision)
    }

    fn sleep(revision: Revision) -> Timer {
        Timer::Sleep(revision)
    }

    fn lookahead(revision: Revision) -> Timer {
        Timer::Lookahead(revision)
    }

    #[test]
    fn a_delay_that_would_overflow_the_clock_is_skipped() {
        let mut timers = Timers::<Timer>::default();

        timers.after(Duration::MAX, toast(revision_after(1)));

        assert_eq!(timers.next_deadline(), None);
    }

    struct TimersRow {
        scheduled: Vec<(u64, Timer)>,
        taken_at: u64,
        fired_timers: Vec<Timer>,
        next_deadline: Option<u64>,
    }

    #[rstest]
    #[case::nothing_fires_before_the_deadline(TimersRow {
        scheduled: vec![(5, toast(revision_after(1)))],
        taken_at: 0,
        fired_timers: Vec::new(),
        next_deadline: Some(5),
    })]
    #[case::rescheduling_a_kind_replaces_its_deadline(TimersRow {
        scheduled: vec![(2, sleep(revision_after(1))), (10, sleep(revision_after(2)))],
        taken_at: 10,
        fired_timers: vec![sleep(revision_after(2))],
        next_deadline: None,
    })]
    #[case::next_deadline_is_the_earlier_of_two_kinds(TimersRow {
        scheduled: vec![(10, sleep(revision_after(1))), (3, toast(revision_after(1)))],
        taken_at: 0,
        fired_timers: Vec::new(),
        next_deadline: Some(3),
    })]
    #[case::a_timer_replaces_only_its_own_kind(TimersRow {
        scheduled: vec![(20, lookahead(revision_after(1))), (5, toast(revision_after(1)))],
        taken_at: 10,
        fired_timers: vec![toast(revision_after(1))],
        next_deadline: Some(20),
    })]
    #[case::due_timers_fire_in_deadline_order_across_kinds(TimersRow {
        scheduled: vec![
            (3, lookahead(revision_after(1))),
            (1, sleep(revision_after(1))),
            (2, toast(revision_after(1))),
        ],
        taken_at: 10,
        fired_timers: vec![
            sleep(revision_after(1)),
            toast(revision_after(1)),
            lookahead(revision_after(1)),
        ],
        next_deadline: None,
    })]
    fn scheduled_timers_fire_once_in_deadline_order(#[case] row: TimersRow) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        let at = |secs| now + Duration::from_secs(secs);
        for (secs, timer) in row.scheduled {
            timers.schedule(at(secs), timer);
        }

        assert_eq!(timers.take_due(at(row.taken_at)), row.fired_timers);
        assert_eq!(timers.take_due(at(row.taken_at)), Vec::new());
        assert_eq!(timers.next_deadline(), row.next_deadline.map(at));
    }
}
