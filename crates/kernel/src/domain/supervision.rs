use std::time::Duration;

use crate::domain::{
    driver::{DriverName, Restarts},
    time::Moment,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Supervision {
    Restart {
        attempts: u8,
        window: Duration,
        announcement: Announcement,
    },
    Degrade(Announcement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Announcement {
    Toast,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Restart,
    Degrade(Announcement),
}

impl Supervision {
    #[must_use]
    pub(crate) const fn standard(driver_name: DriverName) -> Self {
        match driver_name {
            DriverName::Audio => Supervision::Restart {
                attempts: 3,
                window: Duration::from_mins(1),
                announcement: Announcement::Toast,
            },
            DriverName::Library | DriverName::Remote => Supervision::Restart {
                attempts: 1,
                window: Duration::from_mins(1),
                announcement: Announcement::Toast,
            },
            DriverName::Config => Supervision::Degrade(Announcement::Toast),
            DriverName::Macos => Supervision::Degrade(Announcement::Silent),
        }
    }
}

#[must_use]
pub(crate) fn decide_restart(
    supervision: Supervision,
    restarts: &Restarts,
    now: Moment,
) -> Decision {
    match supervision {
        Supervision::Restart {
            attempts,
            window,
            announcement,
        } => {
            if restarts.within(window, now) < usize::from(attempts) {
                Decision::Restart
            } else {
                Decision::Degrade(announcement)
            }
        }
        Supervision::Degrade(announce) => Decision::Degrade(announce),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        driver::{DriverName, Restarts},
        supervision::{Announcement, Supervision},
        time::Moment,
    };

    fn t(secs: u64) -> Moment {
        Moment::new(std::time::Duration::from_secs(secs))
    }

    #[test]
    fn the_standard_strategies_follow_the_plan() {
        assert_eq!(
            Supervision::standard(DriverName::Audio),
            Supervision::Restart {
                attempts: 3,
                window: std::time::Duration::from_secs(60),
                announcement: Announcement::Toast,
            }
        );
        assert_eq!(
            Supervision::standard(DriverName::Library),
            Supervision::Restart {
                attempts: 1,
                window: std::time::Duration::from_secs(60),
                announcement: Announcement::Toast,
            }
        );
        assert_eq!(
            Supervision::standard(DriverName::Config),
            Supervision::Degrade(Announcement::Toast)
        );
        assert_eq!(
            Supervision::standard(DriverName::Macos),
            Supervision::Degrade(Announcement::Silent)
        );
        assert_eq!(
            Supervision::standard(DriverName::Remote),
            Supervision::Restart {
                attempts: 1,
                window: std::time::Duration::from_secs(60),
                announcement: Announcement::Toast,
            }
        );
    }

    #[test]
    fn restarts_keep_at_most_255() {
        let mut restarts = Restarts::default();
        for secs in 0..300u64 {
            restarts.record(t(secs));
        }
        assert_eq!(
            restarts.within(std::time::Duration::from_secs(1000), t(299)),
            255
        );
    }
}
