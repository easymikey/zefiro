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
    pub seek: Revision,
    pub(crate) reported: Revision,
    pub(crate) scrobble: Option<Revision>,
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
