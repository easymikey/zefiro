use std::ops::Deref;

use kernel::domain::appearance::{ProgressBar, Rgb};
use ratatui::style::Color;

use crate::theme::{
    Theme,
    colors::Colors,
    contrast::{MIN_MARKER_CONTRAST, raise_contrast},
    rgb::{ColorDepth, color_at_depth, lerp_rgb, scale_channel},
};

#[derive(Debug, Clone, Copy)]
pub struct ActiveTheme<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) color_depth: ColorDepth,
    fill: Option<Rgb>,
    groove: Option<Rgb>,
    pub volume_pulse_mix: f32,
}

impl<'a> ActiveTheme<'a> {
    #[must_use]
    pub fn new(theme: &'a Theme, color_depth: ColorDepth) -> Self {
        Self {
            theme,
            color_depth,
            fill: None,
            groove: None,
            volume_pulse_mix: 0.0,
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
    pub fn with_volume_pulse(self, volume_pulse_mix: f32) -> Self {
        Self {
            volume_pulse_mix,
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
    pub(crate) fn lifted(&self, rgb: Rgb, toward_text: f32) -> Color {
        self.color(lerp_rgb(rgb, self.theme.colors.foreground, toward_text))
    }

    #[must_use]
    pub(crate) fn spectrum_color_at(&self, fraction: f32) -> Color {
        self.color(self.theme.colors.spectrum_color_at(fraction))
    }

    #[must_use]
    pub(crate) fn muted_accent(&self) -> Color {
        let accent = self.theme.colors.accent.0;
        self.color(Rgb(accent.map(|channel| scale_channel(channel, 0.82))))
    }

    #[must_use]
    pub(crate) fn favorite(&self) -> Color {
        self.color(raise_contrast(
            self.theme.colors.favorite,
            &[
                self.theme.colors.window_background,
                self.theme.colors.selection_background,
            ],
            MIN_MARKER_CONTRAST,
        ))
    }

    #[must_use]
    pub(crate) fn alert(&self) -> Color {
        let [_, _, hot] = self.theme.colors.spectrum;
        self.color(hot)
    }
}

impl<'a> Deref for ActiveTheme<'a> {
    type Target = Theme;
    fn deref(&self) -> &Theme {
        self.theme
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::{ProgressBar, Rgb};
    use ratatui::style::Color;

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
        let accent = active_theme.colors.accent;
        assert_eq!(
            active_theme.color(accent),
            color_at_depth(accent, ColorDepth::Indexed256)
        );
        assert!(matches!(active_theme.color(accent), Color::Indexed(_)));
    }

    #[test]
    fn theme_derefs_to_theme_fields() {
        let theme: Theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        assert_eq!(active_theme.name, active_theme.name);
        assert_eq!(
            active_theme.colors.muted_foreground,
            active_theme.theme.colors.muted_foreground
        );
    }

    #[test]
    fn an_unset_progress_bar_config_is_the_themes_accent_and_groove() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let colors = active_theme.colors();
        assert_eq!(
            (active_theme.progress_fill(), active_theme.progress_groove()),
            (colors.accent, colors.bar_groove)
        );
    }

    #[test]
    fn a_set_progress_bar_config_wins_over_the_theme() {
        let theme = noir();
        let bar = ProgressBar {
            fill: Some(Rgb([255, 0, 0])),
            groove: Some(Rgb([0, 255, 0])),
            ..ProgressBar::default()
        };
        let active_theme =
            ActiveTheme::new(&theme, ColorDepth::TrueColor).with_progress_bar(bar);
        assert_eq!(active_theme.progress_fill(), Color::Rgb(255, 0, 0));
        assert_eq!(active_theme.progress_groove(), Color::Rgb(0, 255, 0));
    }
}
