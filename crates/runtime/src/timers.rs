use std::{mem, time::Instant};

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
    fn a_fresh_scheduler_has_no_deadline() {
        let timers = Timers::<Timer>::default();

        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(revision_after(1)))]
    #[case::sleep(sleep(revision_after(1)))]
    #[case::lookahead(lookahead(revision_after(1)))]
    fn schedule_sets_the_next_deadline(#[case] timer: Timer) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(5), timer);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[rstest]
    #[case::toast(toast(revision_after(1)), toast(revision_after(2)))]
    #[case::lookahead(lookahead(revision_after(1)), lookahead(revision_after(2)))]
    #[case::sleep(sleep(revision_after(1)), sleep(revision_after(2)))]
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

        timers.schedule(now + Duration::from_secs(10), sleep(revision_after(1)));
        timers.schedule(now + Duration::from_secs(3), toast(revision_after(1)));

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(3)));
    }

    #[test]
    fn take_due_returns_nothing_before_the_deadline() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), toast(revision_after(1)));

        assert_eq!(timers.take_due(now), Vec::new());
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[test]
    fn take_due_fires_each_kind_once_in_deadline_order() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), sleep(revision_after(1)));
        timers.schedule(now + Duration::from_secs(1), toast(revision_after(1)));

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(
            fired,
            vec![toast(revision_after(1)), sleep(revision_after(1))]
        );
        assert_eq!(timers.take_due(now + Duration::from_secs(10)), Vec::new());
        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(revision_after(1)), lookahead(revision_after(1)))]
    #[case::sleep(sleep(revision_after(1)), toast(revision_after(1)))]
    #[case::lookahead(lookahead(revision_after(1)), sleep(revision_after(1)))]
    fn a_timer_replaces_only_its_own_kind(
        #[case] timer: Timer,
        #[case] sentinel_timer: Timer,
    ) {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(20), sentinel_timer);
        timers.schedule(now + Duration::from_secs(5), timer);

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![timer]);
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(20)));
    }

    #[test]
    fn take_due_orders_by_deadline_across_kinds() {
        let mut timers = Timers::<Timer>::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(3), lookahead(revision_after(1)));
        timers.schedule(now + Duration::from_secs(1), sleep(revision_after(1)));
        timers.schedule(now + Duration::from_secs(2), toast(revision_after(1)));

        let fired = timers.take_due(now + Duration::from_secs(10));

        assert_eq!(
            fired,
            vec![
                sleep(revision_after(1)),
                toast(revision_after(1)),
                lookahead(revision_after(1))
            ]
        );
    }
}
