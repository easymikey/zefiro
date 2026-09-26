use std::time::Duration;

use crate::domain::{Driver, Moment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervision {
    Restart {
        attempts: u8,
        within: Duration,
        then: Fallback,
    },
    Backoff {
        attempts: u8,
        first: Duration,
        longest: Duration,
        then: Fallback,
    },
    Degrade(Notice),
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    Degrade(Notice),
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    Toast,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Restart,
    RestartAfter(Duration),
    Degrade(Notice),
    Quit,
}

impl From<Fallback> for Decision {
    fn from(fallback: Fallback) -> Self {
        match fallback {
            Fallback::Degrade(notice) => Decision::Degrade(notice),
            Fallback::Fatal => Decision::Quit,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restarts(Vec<Moment>);

impl Restarts {
    pub fn record(&mut self, now: Moment) {
        self.0.push(now);
        if self.0.len() > usize::from(u8::MAX) {
            self.0.remove(0);
        }
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn within(&self, window: Duration, now: Moment) -> usize {
        self.0
            .iter()
            .filter(|moment| now.elapsed_since(**moment) < window)
            .count()
    }
}

impl Supervision {
    #[must_use]
    pub const fn standard(driver: Driver) -> Self {
        match driver {
            Driver::Audio => Supervision::Restart {
                attempts: 3,
                within: Duration::from_secs(60),
                then: Fallback::Degrade(Notice::Toast),
            },
            Driver::Library => Supervision::Restart {
                attempts: 1,
                within: Duration::from_secs(60),
                then: Fallback::Degrade(Notice::Toast),
            },
            Driver::Config => Supervision::Degrade(Notice::Toast),
            Driver::Macos => Supervision::Degrade(Notice::Silent),
        }
    }
}

#[must_use]
pub fn supervise(strategy: Supervision, history: &Restarts, now: Moment) -> Decision {
    match strategy {
        Supervision::Restart {
            attempts,
            within,
            then,
        } => {
            if history.within(within, now) < usize::from(attempts) {
                Decision::Restart
            } else {
                then.into()
            }
        }
        Supervision::Backoff {
            attempts,
            first,
            longest,
            then,
        } => {
            let count = history.count();
            if count < usize::from(attempts) {
                let shift = u32::try_from(count).unwrap_or(u32::MAX);
                let multiplier = 1u32.checked_shl(shift).unwrap_or(u32::MAX);
                Decision::RestartAfter(first.saturating_mul(multiplier).min(longest))
            } else {
                then.into()
            }
        }
        Supervision::Degrade(notice) => Decision::Degrade(notice),
        Supervision::Fatal => Decision::Quit,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        Decision,
        Driver,
        Fallback,
        Moment,
        Notice,
        Restarts,
        Supervision,
        supervise,
    };

    fn t(secs: u64) -> Moment {
        Moment::new(std::time::Duration::from_secs(secs))
    }

    fn history(moments: &[u64]) -> Restarts {
        let mut restarts = Restarts::default();
        for &secs in moments {
            restarts.record(t(secs));
        }
        restarts
    }

    #[rstest]
    #[case::restart_with_no_history(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Fallback::Degrade(Notice::Toast) },
        &[],
        Decision::Restart
    )]
    #[case::restart_under_budget(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Fallback::Degrade(Notice::Toast) },
        &[50, 90],
        Decision::Restart
    )]
    #[case::restart_budget_spent_falls_back(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Fallback::Degrade(Notice::Toast) },
        &[50, 70, 90],
        Decision::Degrade(Notice::Toast)
    )]
    #[case::restart_old_history_ages_out(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Fallback::Degrade(Notice::Toast) },
        &[10, 70, 90],
        Decision::Restart
    )]
    #[case::restart_once_then_fatal(
        Supervision::Restart { attempts: 1, within: std::time::Duration::from_secs(60), then: Fallback::Fatal },
        &[90],
        Decision::Quit
    )]
    #[case::restart_zero_attempts_falls_back(
        Supervision::Restart { attempts: 0, within: std::time::Duration::from_secs(60), then: Fallback::Degrade(Notice::Silent) },
        &[],
        Decision::Degrade(Notice::Silent)
    )]
    #[case::backoff_first_delay(
        Supervision::Backoff { attempts: 3, first: std::time::Duration::from_secs(1), longest: std::time::Duration::from_secs(8), then: Fallback::Degrade(Notice::Toast) },
        &[],
        Decision::RestartAfter(std::time::Duration::from_secs(1))
    )]
    #[case::backoff_doubles(
        Supervision::Backoff { attempts: 3, first: std::time::Duration::from_secs(1), longest: std::time::Duration::from_secs(8), then: Fallback::Degrade(Notice::Toast) },
        &[10, 20],
        Decision::RestartAfter(std::time::Duration::from_secs(4))
    )]
    #[case::backoff_caps_at_longest(
        Supervision::Backoff { attempts: 10, first: std::time::Duration::from_secs(1), longest: std::time::Duration::from_secs(8), then: Fallback::Degrade(Notice::Toast) },
        &[10, 20, 30, 40, 50],
        Decision::RestartAfter(std::time::Duration::from_secs(8))
    )]
    #[case::backoff_spent_falls_back(
        Supervision::Backoff { attempts: 3, first: std::time::Duration::from_secs(1), longest: std::time::Duration::from_secs(8), then: Fallback::Degrade(Notice::Toast) },
        &[10, 20, 30],
        Decision::Degrade(Notice::Toast)
    )]
    #[case::degrade_toast(
        Supervision::Degrade(Notice::Toast),
        &[],
        Decision::Degrade(Notice::Toast)
    )]
    #[case::degrade_silent(
        Supervision::Degrade(Notice::Silent),
        &[],
        Decision::Degrade(Notice::Silent)
    )]
    #[case::fatal(Supervision::Fatal, &[], Decision::Quit)]
    fn supervise_decides_by_strategy(
        #[case] strategy: Supervision,
        #[case] moments: &[u64],
        #[case] expected: Decision,
    ) {
        let now = t(100);
        assert_eq!(supervise(strategy, &history(moments), now), expected);
    }

    #[test]
    fn backoff_huge_count_saturates() {
        let strategy = Supervision::Backoff {
            attempts: 255,
            first: std::time::Duration::from_secs(1),
            longest: std::time::Duration::from_secs(8),
            then: Fallback::Degrade(Notice::Toast),
        };
        let moments: Vec<u64> = (0..64).collect();
        let now = t(100);
        assert_eq!(
            supervise(strategy, &history(&moments), now),
            Decision::RestartAfter(std::time::Duration::from_secs(8))
        );
    }

    #[test]
    fn the_standard_strategies_follow_the_plan() {
        assert_eq!(
            Supervision::standard(Driver::Audio),
            Supervision::Restart {
                attempts: 3,
                within: std::time::Duration::from_secs(60),
                then: Fallback::Degrade(Notice::Toast),
            }
        );
        assert_eq!(
            Supervision::standard(Driver::Library),
            Supervision::Restart {
                attempts: 1,
                within: std::time::Duration::from_secs(60),
                then: Fallback::Degrade(Notice::Toast),
            }
        );
        assert_eq!(
            Supervision::standard(Driver::Config),
            Supervision::Degrade(Notice::Toast)
        );
        assert_eq!(
            Supervision::standard(Driver::Macos),
            Supervision::Degrade(Notice::Silent)
        );
    }

    #[test]
    fn restarts_keep_at_most_255() {
        let mut restarts = Restarts::default();
        for secs in 0..300u64 {
            restarts.record(t(secs));
        }
        assert_eq!(restarts.count(), 255);
        assert_eq!(
            restarts.within(std::time::Duration::from_secs(1000), t(299)),
            255
        );
    }
}
