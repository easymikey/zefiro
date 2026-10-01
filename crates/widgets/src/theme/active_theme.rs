use std::ops::Deref;

use config::{ProgressConfig, Rgb};
use ratatui::style::Color;

use crate::theme::{
    ColorDepth,
    Role,
    Theme,
    contrast::{MIN_MARKER_CONTRAST, raise_contrast},
    rgb::{color_at_depth, lerp_rgb, scale_channel},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BarStyle {
    pub(crate) fill: Color,
    pub(crate) track: Color,
}

impl BarStyle {
    #[must_use]
    pub(crate) fn progress(theme: &ActiveTheme<'_>) -> Self {
        Self {
            fill: theme.color(theme.fill.unwrap_or(theme.colors.role(Role::Accent))),
            track: theme
                .color(theme.track.unwrap_or(theme.colors.role(Role::BarGroove))),
        }
    }

    #[must_use]
    pub(crate) fn volume(theme: &ActiveTheme<'_>) -> Self {
        Self {
            fill: theme.role(Role::Accent),
            track: theme.role(Role::BarGroove),
        }
    }
}

/// Colour contract for themed components: a component's colours come from
/// `XStyle::from_theme`; painters take `&XStyle` and never read roles.
/// Components following it now: progress and volume bars (`BarStyle`), vinyl
/// (`VinylStyle`).
#[derive(Debug, Clone, Copy)]
pub struct ActiveTheme<'a> {
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    fill: Option<Rgb>,
    track: Option<Rgb>,
}

impl<'a> ActiveTheme<'a> {
    #[must_use]
    pub fn new(theme: &'a Theme, color_depth: ColorDepth) -> Self {
        Self {
            theme,
            color_depth,
            fill: None,
            track: None,
        }
    }

    #[must_use]
    pub fn with_progress(self, progress: &ProgressConfig) -> Self {
        Self {
            fill: progress.fill,
            track: progress.track,
            ..self
        }
    }

    #[must_use]
    pub fn color(&self, rgb: Rgb) -> Color {
        color_at_depth(rgb, self.color_depth)
    }

    #[must_use]
    pub fn lifted(&self, rgb: Rgb, toward_text: f32) -> Color {
        self.color(lerp_rgb(rgb, self.colors.role(Role::Text), toward_text))
    }

    #[must_use]
    pub fn spectrum_color_at(&self, t: f32) -> Color {
        self.color(self.theme.colors.spectrum_color_at(t))
    }

    #[must_use]
    pub fn role(&self, role: Role) -> Color {
        self.color(self.colors.role(role))
    }

    #[must_use]
    pub fn muted_accent(&self) -> Color {
        let accent = self.colors.role(Role::Accent).0;
        self.color(Rgb(accent.map(|channel| scale_channel(channel, 0.82))))
    }

    #[must_use]
    pub fn favorite(&self) -> Color {
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
    pub fn alert(&self) -> Color {
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
    use config::Rgb;
    use ratatui::style::Color;

    use crate::theme::{
        ColorDepth,
        Role,
        Theme,
        active_theme::{ActiveTheme, BarStyle},
        color_at_depth,
    };

    fn noir() -> Theme {
        let file =
            config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
                .unwrap();
        Theme::from(file)
    }

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
        assert_eq!(BarStyle::progress(&active), BarStyle::volume(&active));
        assert_eq!(BarStyle::progress(&active).fill, active.role(Role::Accent));
    }

    #[test]
    fn a_set_progress_config_wins_over_the_theme() {
        let theme = noir();
        let progress = config::ProgressConfig {
            fill: Some(Rgb([255, 0, 0])),
            track: Some(Rgb([0, 255, 0])),
            ..config::ProgressConfig::default()
        };
        let active =
            ActiveTheme::new(&theme, ColorDepth::TrueColor).with_progress(&progress);
        assert_eq!(BarStyle::progress(&active).fill, Color::Rgb(255, 0, 0));
        assert_eq!(BarStyle::progress(&active).track, Color::Rgb(0, 255, 0));
        assert_ne!(BarStyle::progress(&active), BarStyle::volume(&active));
    }
}
