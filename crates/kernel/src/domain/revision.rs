#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    pub fn advance(&mut self) {
        *self = self.next();
    }

    pub fn bump(&mut self) -> Self {
        self.advance();
        *self
    }

    pub(crate) fn freshness(self, awaited: Self) -> Freshness {
        if self == awaited {
            Freshness::Awaited
        } else {
            Freshness::Stale
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revisions {
    pub(crate) effects: Revision,
    pub config: Revision,
    pub theme: Revision,
    pub(crate) scan: Revision,
    pub toast: Revision,
    pub sleep: Revision,
    pub lookahead: Revision,
    pub list: Revision,
}

impl Revisions {
    pub(crate) fn issue_effect(&mut self) -> Revision {
        self.effects.bump()
    }

    pub(crate) fn issue_scan(&mut self) -> Revision {
        let issued = self.effects.bump();
        self.scan = issued;
        issued
    }

    pub(crate) fn issue_toast(&mut self) -> Revision {
        let issued = self.effects.bump();
        self.toast = issued;
        issued
    }

    pub(crate) fn issue_lookahead(&mut self) -> Revision {
        let issued = self.effects.bump();
        self.lookahead = issued;
        issued
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    Awaited,
    Stale,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::revision::{Freshness, Revision};

    fn bumped(times: u64) -> Revision {
        (0..times).fold(Revision::default(), |revision, _| revision.next())
    }

    #[rstest]
    #[case::same_generation(2, 2, Freshness::Awaited)]
    #[case::superseded(1, 2, Freshness::Stale)]
    #[case::ahead(3, 2, Freshness::Stale)]
    #[case::untouched(0, 0, Freshness::Awaited)]
    fn only_the_awaited_generation_answers(
        #[case] stamp: u64,
        #[case] awaited: u64,
        #[case] freshness: Freshness,
    ) {
        assert_eq!(bumped(stamp).freshness(bumped(awaited)), freshness);
    }
}
