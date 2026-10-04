use std::ops::Deref;

use kernel::domain::appearance::Rgb;
use ratatui::style::Color;

use crate::{
    appearance::ProgressBar,
    theme::{
        Theme,
        colors::Role,
        contrast::{MIN_MARKER_CONTRAST, raise_contrast},
        rgb::{ColorDepth, color_at_depth, lerp_rgb, scale_channel},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProgressStyle {
    pub(crate) fill: Color,
    pub(crate) groove: Color,
}

impl ProgressStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            fill: theme.color(theme.fill.unwrap_or(theme.colors.role(Role::Accent))),
            groove: theme
                .color(theme.groove.unwrap_or(theme.colors.role(Role::BarGroove))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VolumeStyle {
    pub(crate) fill: Color,
    pub(crate) groove: Color,
}

impl VolumeStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            fill: theme.role(Role::Accent),
            groove: theme.role(Role::BarGroove),
        }
    }
}

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
    pub(crate) fn with_progress(self, progress: ProgressBar) -> Self {
        Self {
            fill: progress.fill,
            groove: progress.groove,
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
    pub(crate) fn lifted(&self, rgb: Rgb, toward_text: f32) -> Color {
        self.color(lerp_rgb(rgb, self.colors.role(Role::Text), toward_text))
    }

    #[must_use]
    pub(crate) fn spectrum_color_at(&self, t: f32) -> Color {
        self.color(self.theme.colors.spectrum_color_at(t))
    }

    #[must_use]
    pub(crate) fn role(&self, role: Role) -> Color {
        self.color(self.colors.role(role))
    }

    #[must_use]
    pub(crate) fn muted_accent(&self) -> Color {
        let accent = self.colors.role(Role::Accent).0;
        self.color(Rgb(accent.map(|channel| scale_channel(channel, 0.82))))
    }

    #[must_use]
    pub(crate) fn favorite(&self) -> Color {
        self.color(raise_contrast(
            self.colors.role(Role::Accent2),
            &[
                self.colors.role(Role::WindowBackground),
                self.colors.role(Role::SelectionBackground),
            ],
            MIN_MARKER_CONTRAST,
        ))
    }

    #[must_use]
    pub(crate) fn alert(&self) -> Color {
        let [_, _, hot] = self.colors.spectrum;
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
    use kernel::domain::appearance::Rgb;
    use ratatui::style::Color;

    use crate::{
        appearance::ProgressBar,
        test_support::noir,
        theme::{
            Theme,
            active_theme::{ActiveTheme, ProgressStyle, VolumeStyle},
            colors::Role,
            rgb::{ColorDepth, color_at_depth},
        },
    };

    #[test]
    fn theme_color_resolves_at_its_own_depth() {
        let theme: Theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::Indexed256);
        let accent = theme.colors.role(Role::Accent);
        assert_eq!(
            theme.color(accent),
            color_at_depth(accent, ColorDepth::Indexed256)
        );
        assert!(matches!(theme.color(accent), Color::Indexed(_)));
    }

    #[test]
    fn theme_derefs_to_theme_fields() {
        let theme: Theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        assert_eq!(theme.name, theme.name);
        assert_eq!(
            theme.colors.role(Role::Frame),
            theme.colors.role(Role::Frame)
        );
    }

    #[test]
    fn an_unset_progress_config_is_the_themes_accent_and_groove() {
        let theme = noir();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let progress = ProgressStyle::from_theme(&active);
        let volume = VolumeStyle::from_theme(&active);
        assert_eq!(
            (progress.fill, progress.groove),
            (volume.fill, volume.groove)
        );
        assert_eq!(progress.fill, active.role(Role::Accent));
    }

    #[test]
    fn a_set_progress_config_wins_over_the_theme() {
        let theme = noir();
        let bar = ProgressBar {
            fill: Some(Rgb([255, 0, 0])),
            groove: Some(Rgb([0, 255, 0])),
            ..ProgressBar::default()
        };
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor).with_progress(bar);
        let progress = ProgressStyle::from_theme(&active);
        let volume = VolumeStyle::from_theme(&active);
        assert_eq!(progress.fill, Color::Rgb(255, 0, 0));
        assert_eq!(progress.groove, Color::Rgb(0, 255, 0));
        assert_ne!(
            (progress.fill, progress.groove),
            (volume.fill, volume.groove)
        );
    }
}
