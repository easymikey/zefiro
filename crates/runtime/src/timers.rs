use std::{mem, time::Instant};

#[derive(Debug)]
struct Scheduled<M> {
    deadline: Instant,
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
    pub(crate) fn schedule(&mut self, deadline: Instant, message: M) {
        self.scheduled.retain(|held| {
            mem::discriminant(&held.message) != mem::discriminant(&message)
        });
        self.scheduled.push(Scheduled { deadline, message });
    }

    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.scheduled
            .iter()
            .map(|scheduled| scheduled.deadline)
            .min()
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<M> {
        self.scheduled.sort_by_key(|scheduled| scheduled.deadline);
        let due = self
            .scheduled
            .partition_point(|scheduled| scheduled.deadline <= now);
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

    fn toast(revision: u64) -> Timer {
        Timer::Toast(
            (0..revision).fold(Revision::default(), |revision, _| revision.next()),
        )
    }

    fn sleep(revision: u64) -> Timer {
        Timer::Sleep(
            (0..revision).fold(Revision::default(), |revision, _| revision.next()),
        )
    }

    fn lookahead(revision: u64) -> Timer {
        Timer::Lookahead(
            (0..revision).fold(Revision::default(), |revision, _| revision.next()),
        )
    }

    #[test]
    fn a_fresh_scheduler_has_no_deadline() {
        let timers = Timers::<Timer>::default();

        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(1))]
    #[case::sleep(sleep(1))]
    #[case::lookahead(lookahead(1))]
    fn schedule_sets_the_next_deadline(#[case] message: Timer) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(5), message);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[rstest]
    #[case::toast(toast(1), toast(2))]
    #[case::lookahead(lookahead(1), lookahead(2))]
    #[case::sleep(sleep(1), sleep(2))]
    fn rescheduling_a_kind_replaces_its_deadline(
        #[case] first: Timer,
        #[case] second: Timer,
    ) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(10), first);
        timers.schedule(now + Duration::from_secs(2), second);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(2)));
    }

    #[test]
    fn next_deadline_is_the_earlier_of_the_two_kinds() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(10), sleep(1));
        timers.schedule(now + Duration::from_secs(3), toast(1));

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(3)));
    }

    #[test]
    fn take_due_returns_nothing_before_the_deadline() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), toast(1));

        assert_eq!(timers.take_due(now), Vec::new());
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[test]
    fn take_due_fires_each_kind_once_in_deadline_order() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), sleep(1));
        timers.schedule(now + Duration::from_secs(1), toast(1));

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![toast(1), sleep(1)]);
        assert_eq!(timers.take_due(now + Duration::from_secs(10)), Vec::new());
        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(1), lookahead(1))]
    #[case::sleep(sleep(1), toast(1))]
    #[case::lookahead(lookahead(1), sleep(1))]
    fn a_timer_replaces_only_its_own_kind(
        #[case] message: Timer,
        #[case] sentinel: Timer,
    ) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(20), sentinel);
        timers.schedule(now + Duration::from_secs(5), message);

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![message]);
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(20)));
    }

    #[test]
    fn take_due_orders_by_deadline_across_kinds() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(3), lookahead(1));
        timers.schedule(now + Duration::from_secs(1), sleep(1));
        timers.schedule(now + Duration::from_secs(2), toast(1));

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![sleep(1), toast(1), lookahead(1)]);
    }
}
