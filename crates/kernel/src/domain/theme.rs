use std::{borrow::Cow, fmt, str::FromStr};

use crate::domain::direction::Direction;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThemeName(Cow<'static, str>);

impl ThemeName {
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    pub fn new(name: String) -> Result<Self, ThemeNameError> {
        if name.is_empty() {
            Err(ThemeNameError::Empty)
        } else if name == "auto" {
            Err(ThemeNameError::Reserved)
        } else {
            Ok(Self(Cow::Owned(name)))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<ThemeName> for &str {
    fn eq(&self, other: &ThemeName) -> bool {
        *self == other.as_str()
    }
}

impl fmt::Display for ThemeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ThemeNameError {
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
    type Err = ThemeNameError;

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
    pub(crate) fn stepped(&self, direction: Direction) -> Option<ThemeName> {
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
            || match direction {
                Direction::Next => 0,
                Direction::Previous => self.names.len() - 1,
            },
            |index| direction.wrapped(index, self.names.len()),
        );
        self.names.get(next).cloned()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        direction::Direction,
        theme::{ThemeChoice, ThemeName, ThemeNameError, Themes},
    };

    fn names(values: &[&'static str]) -> Vec<ThemeName> {
        values.iter().copied().map(ThemeName::from_static).collect()
    }

    struct StepRow {
        themes: Themes,
        direction: Direction,
        expected: Option<&'static str>,
    }

    #[rstest]
    #[case::next(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("a")) },
        direction: Direction::Next,
        expected: Some("b"),
    })]
    #[case::wraps_at_end(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("c")) },
        direction: Direction::Next,
        expected: Some("a"),
    })]
    #[case::previous_wraps_at_start(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("a")) },
        direction: Direction::Previous,
        expected: Some("c"),
    })]
    #[case::selected_missing_from_list_up(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("gone")) },
        direction: Direction::Next,
        expected: Some("a"),
    })]
    #[case::selected_missing_from_list_down(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Named(ThemeName::from_static("gone")) },
        direction: Direction::Previous,
        expected: Some("c"),
    })]
    #[case::auto_up(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Auto },
        direction: Direction::Next,
        expected: Some("a"),
    })]
    #[case::auto_down(StepRow {
        themes: Themes { names: names(&["a", "b", "c"]), selected: ThemeChoice::Auto },
        direction: Direction::Previous,
        expected: Some("c"),
    })]
    #[case::empty_list_gives_none(StepRow {
        themes: Themes { names: Vec::new(), selected: ThemeChoice::Auto },
        direction: Direction::Next,
        expected: None,
    })]
    fn themes_step_onto_a_listed_name(#[case] row: StepRow) {
        assert_eq!(
            row.themes
                .stepped(row.direction)
                .map(|name| name.as_str().to_string()),
            row.expected.map(str::to_string)
        );
    }

    #[rstest]
    #[case::auto("auto", Ok(ThemeChoice::Auto))]
    #[case::named("noir", Ok(ThemeChoice::Named(ThemeName::from_static("noir"))))]
    #[case::empty("", Err(ThemeNameError::Empty))]
    #[case::case_sensitive(
        "AUTO",
        Ok(ThemeChoice::Named(ThemeName::from_static("AUTO")))
    )]
    fn theme_choice_parses(
        #[case] text: &str,
        #[case] expected: Result<ThemeChoice, ThemeNameError>,
    ) {
        assert_eq!(text.parse::<ThemeChoice>(), expected);
    }
}
