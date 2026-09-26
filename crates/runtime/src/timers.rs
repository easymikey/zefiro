use std::time::Instant;

use kernel::{SleepTimer, Timer, domain::Driver};

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    deadline: Instant,
    message: Timer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerSlot {
    Toast,
    Sleep,
    Mark,
    Restart(Driver),
}

impl TimerSlot {
    const COUNT: usize = 3 + Driver::ALL.len();

    const fn index(self) -> usize {
        match self {
            TimerSlot::Toast => 0,
            TimerSlot::Sleep => 1,
            TimerSlot::Mark => 2,
            TimerSlot::Restart(driver) => 3 + driver.index(),
        }
    }
}

impl From<Timer> for TimerSlot {
    fn from(timer: Timer) -> Self {
        match timer {
            Timer::Toast(_) => TimerSlot::Toast,
            Timer::Sleep(_) => TimerSlot::Sleep,
            Timer::Mark(_) => TimerSlot::Mark,
            Timer::Restart(driver) => TimerSlot::Restart(driver),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Timers {
    scheduled: [Option<Scheduled>; TimerSlot::COUNT],
}

impl Timers {
    pub(crate) fn schedule(&mut self, deadline: Instant, message: Timer) {
        let slot = TimerSlot::from(message).index();
        if let Some(entry) = self.scheduled.get_mut(slot) {
            *entry = Some(Scheduled { deadline, message });
        }
    }

    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.scheduled
            .iter()
            .copied()
            .flatten()
            .map(|scheduled| scheduled.deadline)
            .min()
    }

    pub(crate) fn due(&mut self, now: Instant) -> Vec<Timer> {
        let mut fired = Vec::new();
        for slot in &mut self.scheduled {
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
        let deadline = self
            .scheduled
            .get(TimerSlot::Sleep.index())
            .copied()
            .flatten()
            .map(|scheduled| scheduled.deadline);
        sleep.and(deadline)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::{
        SleepTimer,
        Timer,
        domain::{Driver, Revision},
    };
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

    fn mark(revision: u64) -> Timer {
        Timer::Mark(
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
    #[case::restart(Timer::Restart(Driver::Audio))]
    fn schedule_sets_the_next_deadline(#[case] message: Timer) {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(5), message);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[rstest]
    #[case::toast(toast(1), toast(2))]
    #[case::mark(mark(1), mark(2))]
    #[case::restart(Timer::Restart(Driver::Library), Timer::Restart(Driver::Library))]
    fn rescheduling_a_slot_replaces_its_deadline(
        #[case] first: Timer,
        #[case] second: Timer,
    ) {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(10), first);
        timers.schedule(now + Duration::from_secs(2), second);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(2)));
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

    #[test]
    fn restarts_of_two_drivers_keep_separate_slots() {
        let mut timers = Timers::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(1), Timer::Restart(Driver::Audio));
        timers.schedule(
            now + Duration::from_secs(1),
            Timer::Restart(Driver::Library),
        );

        let fired = timers.due(now + Duration::from_secs(5));

        assert_eq!(
            fired,
            vec![
                Timer::Restart(Driver::Audio),
                Timer::Restart(Driver::Library)
            ]
        );
        assert_eq!(timers.next_deadline(), None);
    }

    #[rstest]
    #[case::toast(toast(1))]
    #[case::sleep(sleep(1))]
    #[case::mark(mark(1))]
    #[case::restart_audio(Timer::Restart(Driver::Audio))]
    #[case::restart_macos(Timer::Restart(Driver::Macos))]
    fn a_timer_lands_in_its_own_slot(#[case] message: Timer) {
        let mut timers = Timers::default();
        let now = Instant::now();
        let sentinel = Timer::Restart(Driver::Config);

        timers.schedule(now + Duration::from_secs(20), sentinel);
        timers.schedule(now + Duration::from_secs(5), message);

        let fired = timers.due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![message]);
        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(20)));
    }

    #[test]
    fn due_orders_by_deadline_across_slots() {
        let mut timers = Timers::default();
        let now = Instant::now();
        timers.schedule(now + Duration::from_secs(3), mark(1));
        timers.schedule(now + Duration::from_secs(1), Timer::Restart(Driver::Audio));
        timers.schedule(now + Duration::from_secs(2), toast(1));

        let fired = timers.due(now + Duration::from_secs(10));

        assert_eq!(
            fired,
            vec![Timer::Restart(Driver::Audio), toast(1), mark(1)]
        );
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
