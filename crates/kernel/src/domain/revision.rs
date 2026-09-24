#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    pub const UNSTAMPED: Self = Self(0);

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    pub fn bump(&mut self) -> Self {
        *self = self.next();
        *self
    }

    pub fn delivery(self, performed: Self) -> Delivery {
        if self > performed {
            Delivery::Fresh
        } else {
            Delivery::Replay
        }
    }

    pub fn reply(self, awaited: Self) -> Reply {
        if self == awaited {
            Reply::Awaited
        } else {
            Reply::Stale
        }
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Fresh,
    Replay,
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Awaited,
    Stale,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::revision::{Delivery, Reply, Revision};

    fn bumped(times: u64) -> Revision {
        (0..times).fold(Revision::default(), |revision, _| revision.next())
    }

    #[rstest]
    #[case::above(1, 0, Delivery::Fresh)]
    #[case::equal(1, 1, Delivery::Replay)]
    #[case::below(1, 2, Delivery::Replay)]
    #[case::untouched(0, 0, Delivery::Replay)]
    fn a_stamp_no_higher_than_the_performed_one_is_a_replay(
        #[case] stamp: u64,
        #[case] performed: u64,
        #[case] delivery: Delivery,
    ) {
        assert_eq!(bumped(stamp).delivery(bumped(performed)), delivery);
    }

    #[rstest]
    #[case::same_generation(2, 2, Reply::Awaited)]
    #[case::superseded(1, 2, Reply::Stale)]
    #[case::ahead(3, 2, Reply::Stale)]
    #[case::untouched(0, 0, Reply::Awaited)]
    fn only_the_awaited_generation_answers(
        #[case] stamp: u64,
        #[case] awaited: u64,
        #[case] reply: Reply,
    ) {
        assert_eq!(bumped(stamp).reply(bumped(awaited)), reply);
    }
}
