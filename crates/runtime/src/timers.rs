use std::time::Instant;

use kernel::{SleepTimer, Timer};

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    deadline: Instant,
    message: Timer,
}

#[derive(Debug, Default)]
pub(crate) struct Timers {
    toast: Option<Scheduled>,
    sleep: Option<Scheduled>,
}

impl Timers {
    pub(crate) fn schedule(&mut self, deadline: Instant, message: Timer) {
        let scheduled = Scheduled { deadline, message };
        match message {
            Timer::Toast(_) => self.toast = Some(scheduled),
            Timer::Sleep(_) => self.sleep = Some(scheduled),
        }
    }

    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        [self.toast, self.sleep]
            .into_iter()
            .flatten()
            .map(|scheduled| scheduled.deadline)
            .min()
    }

    pub(crate) fn due(&mut self, now: Instant) -> Vec<Timer> {
        let mut fired = Vec::new();
        for slot in [&mut self.toast, &mut self.sleep] {
            match *slot {
                Some(scheduled) if scheduled.deadline <= now => {
                    fired.push(scheduled);
                    *slot = None;
                }
                Some(_) | None => {}
            }
        }
        fired.sort_by_key(|scheduled| scheduled.deadline);
        fired
            .into_iter()
            .map(|scheduled| scheduled.message)
            .collect()
    }

    #[must_use]
    pub(crate) fn sleep_deadline(&self, sleep: Option<SleepTimer>) -> Option<Instant> {
        sleep.and(self.sleep.map(|scheduled| scheduled.deadline))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::{SleepTimer, Timer, domain::Revision};
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

    #[test]
    fn a_fresh_scheduler_has_no_deadline() {
        let timers = Timers::default();

        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(1))]
    #[case::sleep(sleep(1))]
    fn schedule_sets_the_next_deadline(#[case] message: Timer) {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(5), message);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[rstest]
    #[case::a_later_schedule_wins(Duration::from_secs(2), Duration::from_secs(10))]
    #[case::an_earlier_schedule_wins(Duration::from_secs(10), Duration::from_secs(2))]
    fn a_second_schedule_of_the_same_kind_replaces_the_first(
        #[case] first_delay: Duration,
        #[case] second_delay: Duration,
    ) {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + first_delay, sleep(1));
        timers.schedule(now + second_delay, sleep(2));

        assert_eq!(timers.next_deadline(), Some(now + second_delay));
    }

    #[test]
    fn next_deadline_is_the_earlier_of_the_two_kinds() {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(10), sleep(1));
        timers.schedule(now + Duration::from_secs(3), toast(1));

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(3)));
    }

    #[test]
    fn due_returns_nothing_before_the_deadline() {
        let mut timers = Timers::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), toast(1));

        assert_eq!(timers.due(now), Vec::new());
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[test]
    fn due_fires_each_kind_once_in_deadline_order() {
        let mut timers = Timers::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(5), sleep(1));
        timers.schedule(now + Duration::from_secs(1), toast(1));

        let fired = timers.due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![toast(1), sleep(1)]);
        assert_eq!(timers.due(now + Duration::from_secs(10)), Vec::new());
        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::armed_and_still_pending(Some(SleepTimer { preset_index: 0, delay: Duration::from_secs(60) }), true, true)]
    #[case::armed_but_never_scheduled(Some(SleepTimer { preset_index: 0, delay: Duration::from_secs(60) }), false, false)]
    #[case::scheduled_but_cycled_off(None, true, false)]
    #[case::disarmed_and_never_scheduled(None, false, false)]
    fn sleep_deadline_is_gated_by_the_model(
        #[case] model_sleep: Option<SleepTimer>,
        #[case] scheduled: bool,
        #[case] expects_a_deadline: bool,
    ) {
        let mut timers = Timers::default();
        let now = Instant::now();
        if scheduled {
            timers.schedule(now + Duration::from_secs(30), sleep(1));
        }

        let deadline = timers.sleep_deadline(model_sleep);

        assert_eq!(deadline.is_some(), expects_a_deadline);
    }
}
