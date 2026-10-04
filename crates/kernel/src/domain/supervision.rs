use std::time::Duration;

use crate::domain::{DriverName, Moment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervision {
    Restart {
        attempts: u8,
        within: Duration,
        then: Announce,
    },
    Degrade(Announce),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Announce {
    Toast,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Restart,
    Degrade(Announce),
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
    pub const fn standard(driver: DriverName) -> Self {
        match driver {
            DriverName::Audio => Supervision::Restart {
                attempts: 3,
                within: Duration::from_secs(60),
                then: Announce::Toast,
            },
            DriverName::Library => Supervision::Restart {
                attempts: 1,
                within: Duration::from_secs(60),
                then: Announce::Toast,
            },
            DriverName::Config => Supervision::Degrade(Announce::Toast),
            DriverName::Macos => Supervision::Degrade(Announce::Silent),
        }
    }
}

#[must_use]
pub fn decide_restart(
    strategy: Supervision,
    restarts: &Restarts,
    now: Moment,
) -> Decision {
    match strategy {
        Supervision::Restart {
            attempts,
            within,
            then,
        } => {
            if restarts.within(within, now) < usize::from(attempts) {
                Decision::Restart
            } else {
                Decision::Degrade(then)
            }
        }
        Supervision::Degrade(announce) => Decision::Degrade(announce),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        Announce,
        Decision,
        DriverName,
        Moment,
        Restarts,
        Supervision,
        decide_restart,
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
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Announce::Toast },
        &[],
        Decision::Restart
    )]
    #[case::restart_under_budget(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Announce::Toast },
        &[50, 90],
        Decision::Restart
    )]
    #[case::restart_budget_spent_falls_back(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Announce::Toast },
        &[50, 70, 90],
        Decision::Degrade(Announce::Toast)
    )]
    #[case::restart_old_history_ages_out(
        Supervision::Restart { attempts: 3, within: std::time::Duration::from_secs(60), then: Announce::Toast },
        &[10, 70, 90],
        Decision::Restart
    )]
    #[case::restart_zero_attempts_falls_back(
        Supervision::Restart { attempts: 0, within: std::time::Duration::from_secs(60), then: Announce::Silent },
        &[],
        Decision::Degrade(Announce::Silent)
    )]
    #[case::degrade_toast(
        Supervision::Degrade(Announce::Toast),
        &[],
        Decision::Degrade(Announce::Toast)
    )]
    #[case::degrade_silent(
        Supervision::Degrade(Announce::Silent),
        &[],
        Decision::Degrade(Announce::Silent)
    )]
    fn decide_restart_decides_by_strategy(
        #[case] strategy: Supervision,
        #[case] moments: &[u64],
        #[case] expected: Decision,
    ) {
        let now = t(100);
        assert_eq!(decide_restart(strategy, &history(moments), now), expected);
    }

    #[test]
    fn the_standard_strategies_follow_the_plan() {
        assert_eq!(
            Supervision::standard(DriverName::Audio),
            Supervision::Restart {
                attempts: 3,
                within: std::time::Duration::from_secs(60),
                then: Announce::Toast,
            }
        );
        assert_eq!(
            Supervision::standard(DriverName::Library),
            Supervision::Restart {
                attempts: 1,
                within: std::time::Duration::from_secs(60),
                then: Announce::Toast,
            }
        );
        assert_eq!(
            Supervision::standard(DriverName::Config),
            Supervision::Degrade(Announce::Toast)
        );
        assert_eq!(
            Supervision::standard(DriverName::Macos),
            Supervision::Degrade(Announce::Silent)
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
