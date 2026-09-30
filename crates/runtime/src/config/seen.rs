use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Signature(u64);

impl Signature {
    fn of(text: &str) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Seen {
    #[default]
    Absent,
    Never,
    Content(Signature),
}

impl Seen {
    #[must_use]
    pub(crate) fn of(text: Option<&str>) -> Self {
        text.map_or(Seen::Absent, |text| Seen::Content(Signature::of(text)))
    }

    #[must_use]
    pub(crate) fn starting(text: Option<&str>) -> Self {
        text.map_or(Seen::Never, |text| Seen::Content(Signature::of(text)))
    }

    #[must_use]
    pub(crate) fn changed_by(self, text: Option<&str>) -> bool {
        match self {
            Seen::Never => true,
            Seen::Absent | Seen::Content(_) => self != Seen::of(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::config::seen::Seen;

    proptest! {
        #[test]
        fn a_fresh_target_always_reports_changed(text in proptest::option::of(".*")) {
            prop_assert!(Seen::Never.changed_by(text.as_deref()));
        }

        #[test]
        fn its_own_text_never_reports_changed(text in proptest::option::of(".*")) {
            let seen = Seen::of(text.as_deref());
            prop_assert!(!seen.changed_by(text.as_deref()));
        }

        #[test]
        fn a_different_text_reports_changed(
            first in proptest::option::of(".*"),
            second in proptest::option::of(".*"),
        ) {
            prop_assume!(first != second);
            let seen = Seen::of(first.as_deref());
            prop_assert!(seen.changed_by(second.as_deref()));
        }
    }
}
