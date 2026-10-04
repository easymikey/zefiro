use std::time::Instant;

use kernel::{SleepTimer, Timer};

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    deadline: Instant,
    timer: Timer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerName {
    Toast,
    Sleep,
    Lookahead,
}

impl TimerName {
    const COUNT: usize = 3;

    const fn index(self) -> usize {
        match self {
            TimerName::Toast => 0,
            TimerName::Sleep => 1,
            TimerName::Lookahead => 2,
        }
    }
}

impl From<Timer> for TimerName {
    fn from(timer: Timer) -> Self {
        match timer {
            Timer::Toast(_) => TimerName::Toast,
            Timer::Sleep(_) => TimerName::Sleep,
            Timer::Lookahead(_) => TimerName::Lookahead,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Timers {
    scheduled: [Option<Scheduled>; TimerName::COUNT],
}

impl Timers {
    pub(crate) fn schedule(&mut self, deadline: Instant, message: Timer) {
        let slot = TimerName::from(message).index();
        if let Some(entry) = self.scheduled.get_mut(slot) {
            *entry = Some(Scheduled {
                deadline,
                timer: message,
            });
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
        let mut fired: Vec<_> = self
            .scheduled
            .iter_mut()
            .filter_map(|slot| slot.take_if(|scheduled| scheduled.deadline <= now))
            .collect();
        fired.sort_by_key(|scheduled| scheduled.deadline);
        fired.into_iter().map(|scheduled| scheduled.timer).collect()
    }

    #[must_use]
    pub(crate) fn sleep_deadline(&self, sleep: Option<SleepTimer>) -> Option<Instant> {
        let deadline = self
            .scheduled
            .get(TimerName::Sleep.index())
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
        domain::{PresetIndex, Revision},
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

    fn lookahead(revision: u64) -> Timer {
        Timer::Lookahead(
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
    #[case::lookahead(lookahead(1))]
    fn schedule_sets_the_next_deadline(#[case] message: Timer) {
        let mut timers = Timers::default();
        let now = Instant::now();

        timers.schedule(now + Duration::from_secs(5), message);

        assert_eq!(timers.next_deadline(), Some(now + Duration::from_secs(5)));
    }

    #[rstest]
    #[case::toast(toast(1), toast(2))]
    #[case::lookahead(lookahead(1), lookahead(2))]
    #[case::sleep(sleep(1), sleep(2))]
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

    #[rstest]
    #[case::toast(toast(1), lookahead(1))]
    #[case::sleep(sleep(1), toast(1))]
    #[case::lookahead(lookahead(1), sleep(1))]
    fn a_timer_lands_in_its_own_slot(#[case] message: Timer, #[case] sentinel: Timer) {
        let mut timers = Timers::default();
        let now = Instant::now();

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
        timers.schedule(now + Duration::from_secs(3), lookahead(1));
        timers.schedule(now + Duration::from_secs(1), sleep(1));
        timers.schedule(now + Duration::from_secs(2), toast(1));

        let fired = timers.due(now + Duration::from_secs(10));

        assert_eq!(fired, vec![sleep(1), toast(1), lookahead(1)]);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Scheduled {
        Yes,
        No,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Deadline {
        Expected,
        Absent,
    }

    #[derive(Debug)]
    struct SleepDeadlineRow {
        model_sleep: Option<SleepTimer>,
        scheduled: Scheduled,
        deadline: Deadline,
    }

    const ARMED: Option<SleepTimer> = Some(SleepTimer {
        preset_index: PresetIndex::new(0),
        delay: Duration::from_secs(60),
    });

    #[rstest]
    #[case::armed_and_still_pending(SleepDeadlineRow { model_sleep: ARMED, scheduled: Scheduled::Yes, deadline: Deadline::Expected })]
    #[case::armed_but_never_scheduled(SleepDeadlineRow { model_sleep: ARMED, scheduled: Scheduled::No, deadline: Deadline::Absent })]
    #[case::scheduled_but_cycled_off(SleepDeadlineRow { model_sleep: None, scheduled: Scheduled::Yes, deadline: Deadline::Absent })]
    #[case::disarmed_and_never_scheduled(SleepDeadlineRow { model_sleep: None, scheduled: Scheduled::No, deadline: Deadline::Absent })]
    fn sleep_deadline_is_gated_by_the_model(#[case] row: SleepDeadlineRow) {
        let mut timers = Timers::default();
        let now = Instant::now();
        if row.scheduled == Scheduled::Yes {
            timers.schedule(now + Duration::from_secs(30), sleep(1));
        }

        let deadline = match timers.sleep_deadline(row.model_sleep) {
            Some(_) => Deadline::Expected,
            None => Deadline::Absent,
        };

        assert_eq!(deadline, row.deadline);
    }
}
