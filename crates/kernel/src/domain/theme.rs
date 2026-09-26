use std::{borrow::Cow, fmt, str::FromStr};

use crate::domain::Nudge;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThemeName(Cow<'static, str>);

impl ThemeName {
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    pub fn new(name: String) -> Result<Self, ThemeNameRejection> {
        if name.is_empty() {
            Err(ThemeNameRejection::Empty)
        } else if name == "auto" {
            Err(ThemeNameRejection::Reserved)
        } else {
            Ok(Self(Cow::Owned(name)))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThemeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ThemeNameRejection {
    #[error("a theme name cannot be empty")]
    Empty,
    #[error("\"auto\" is a reserved theme name")]
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    Auto,
    Named(ThemeName),
}

impl FromStr for ThemeChoice {
    type Err = ThemeNameRejection;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text == "auto" {
            Ok(ThemeChoice::Auto)
        } else {
            Ok(ThemeChoice::Named(ThemeName::new(text.to_string())?))
        }
    }
}

impl fmt::Display for ThemeChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeChoice::Auto => formatter.write_str("auto"),
            ThemeChoice::Named(name) => name.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Themes {
    pub names: Vec<ThemeName>,
    pub selected: ThemeChoice,
}

impl Themes {
    #[must_use]
    pub fn nudged(&self, nudge: Nudge) -> Option<ThemeName> {
        if self.names.is_empty() {
            return None;
        }
        let current = match &self.selected {
            ThemeChoice::Named(name) => {
                self.names.iter().position(|candidate| candidate == name)
            }
            ThemeChoice::Auto => None,
        };
        let next = current.map_or_else(
            || match nudge {
                Nudge::Up => 0,
                Nudge::Down => self.names.len() - 1,
            },
            |index| wrapped(index, self.names.len(), nudge),
        );
        self.names.get(next).cloned()
    }
}

fn wrapped(current: usize, len: usize, nudge: Nudge) -> usize {
    let delta: isize = match nudge {
        Nudge::Up => 1,
        Nudge::Down => -1,
    };
    let (Ok(len_signed), Ok(current_signed)) =
        (isize::try_from(len.max(1)), isize::try_from(current))
    else {
        return current;
    };
    let wrapped = (current_signed + delta).rem_euclid(len_signed);
    usize::try_from(wrapped).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{Nudge, ThemeChoice, ThemeName, ThemeNameRejection, Themes};

    fn names(values: &[&'static str]) -> Vec<ThemeName> {
        values.iter().copied().map(ThemeName::from_static).collect()
    }

    struct NudgeRow {
        themes: Themes,
        nudge: Nudge,
        expected: Option<&'static str>,
    }

    #[rstest]
    #[case::next(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("a")) },
        nudge: Nudge::Up,
        expected: Some("b"),
    })]
    #[case::wraps_at_end(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("c")) },
        nudge: Nudge::Up,
        expected: Some("a"),
    })]
    #[case::previous_wraps_at_start(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("a")) },
        nudge: Nudge::Down,
        expected: Some("c"),
    })]
    #[case::selected_missing_from_list_up(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("gone")) },
        nudge: Nudge::Up,
        expected: Some("a"),
    })]
    #[case::selected_missing_from_list_down(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("gone")) },
        nudge: Nudge::Down,
        expected: Some("c"),
    })]
    #[case::auto_up(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Auto },
        nudge: Nudge::Up,
        expected: Some("a"),
    })]
    #[case::auto_down(NudgeRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Auto },
        nudge: Nudge::Down,
        expected: Some("c"),
    })]
    #[case::empty_list_gives_none(NudgeRow {
        themes: Themes { names: Vec::new(), selected: ThemeChoice::Auto },
        nudge: Nudge::Up,
        expected: None,
    })]
    fn themes_nudge_onto_a_listed_name(#[case] row: NudgeRow) {
        assert_eq!(
            row.themes
                .nudged(row.nudge)
                .map(|name| name.as_str().to_string()),
            row.expected.map(str::to_string)
        );
    }

    #[rstest]
    #[case::auto("auto", Ok(ThemeChoice::Auto))]
    #[case::named("noir", Ok(ThemeChoice::Named(ThemeName::from_static("noir"))))]
    #[case::empty("", Err(ThemeNameRejection::Empty))]
    #[case::case_sensitive(
        "AUTO",
        Ok(ThemeChoice::Named(ThemeName::from_static("AUTO")))
    )]
    fn theme_choice_parses(
        #[case] text: &str,
        #[case] expected: Result<ThemeChoice, ThemeNameRejection>,
    ) {
        assert_eq!(text.parse::<ThemeChoice>(), expected);
    }
}
