use kernel::domain::appearance::{ProgressBar, Rgb};
use ratatui::style::Color;

use crate::{
    primitive::spinner::Spinner,
    theme::{
        Theme,
        colors::Colors,
        rgb::{ColorDepth, color_at_depth, shade},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct ActiveTheme<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) color_depth: ColorDepth,
    pub(crate) fill: Option<Rgb>,
    pub(crate) groove: Option<Rgb>,
    pub(crate) spinner: Spinner,
}

impl<'a> ActiveTheme<'a> {
    #[must_use]
    pub fn new(theme: &'a Theme, color_depth: ColorDepth) -> Self {
        Self {
            theme,
            color_depth,
            fill: None,
            groove: None,
            spinner: Spinner::default(),
        }
    }

    #[must_use]
    pub(crate) fn with_progress_bar(self, progress_bar: ProgressBar) -> Self {
        Self {
            fill: progress_bar.fill,
            groove: progress_bar.groove,
            ..self
        }
    }

    #[must_use]
    pub(crate) fn color(&self, rgb: Rgb) -> Color {
        color_at_depth(rgb, self.color_depth)
    }

    #[must_use]
    pub fn colors(&self) -> Colors<Color> {
        self.theme.colors.map(|rgb| self.color(rgb))
    }

    #[must_use]
    pub(crate) fn progress_fill(&self) -> Color {
        self.color(self.fill.unwrap_or(self.theme.colors.accent))
    }

    #[must_use]
    pub(crate) fn progress_groove(&self) -> Color {
        self.color(self.groove.unwrap_or(self.theme.colors.bar_groove))
    }

    #[must_use]
    pub(crate) fn spectrum_color_at(&self, fraction: f32) -> Color {
        self.color(self.theme.colors.spectrum_color_at(fraction))
    }

    #[must_use]
    pub(crate) fn muted_accent(&self) -> Color {
        self.color(shade(self.theme.colors.accent, 0.82))
    }

    #[must_use]
    pub(crate) fn alert(&self) -> Color {
        let [_, _, hot] = self.theme.colors.spectrum;
        self.color(hot)
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::{ProgressBar, Rgb};
    use ratatui::style::Color;
    use rstest::rstest;

    use crate::{
        test_support::noir,
        theme::{
            Theme,
            active_theme::ActiveTheme,
            rgb::{ColorDepth, color_at_depth},
        },
    };

    #[test]
    fn theme_color_resolves_at_its_own_depth() {
        let theme: Theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::Indexed256);
        let accent = theme.colors.accent;
        assert_eq!(
            active_theme.color(accent),
            color_at_depth(accent, ColorDepth::Indexed256)
        );
        assert!(matches!(active_theme.color(accent), Color::Indexed(_)));
    }

    #[rstest]
    #[case::unset_is_the_themes_accent_and_groove(
        ProgressBar::default(),
        (
            color_at_depth(noir().colors.accent, ColorDepth::TrueColor),
            color_at_depth(noir().colors.bar_groove, ColorDepth::TrueColor),
        )
    )]
    #[case::set_wins_over_the_theme(
        ProgressBar {
            fill: Some(Rgb([255, 0, 0])),
            groove: Some(Rgb([0, 255, 0])),
            ..ProgressBar::default()
        },
        (Color::Rgb(255, 0, 0), Color::Rgb(0, 255, 0))
    )]
    fn a_progress_bar_config_wins_over_the_theme_only_when_set(
        #[case] bar: ProgressBar,
        #[case] expected: (Color, Color),
    ) {
        let theme = noir();
        let active_theme =
            ActiveTheme::new(&theme, ColorDepth::TrueColor).with_progress_bar(bar);
        assert_eq!(
            (active_theme.progress_fill(), active_theme.progress_groove()),
            expected
        );
    }
}
