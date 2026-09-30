use std::ops::Deref;

use config::Rgb;
use raster::{BarColorOverrides, BarColors};
use ratatui::style::Color;

use crate::theme::{
    ColorDepth,
    Role,
    Theme,
    bars::{FillColors, bar_colors},
    contrast::{MIN_MARKER_CONTRAST, raise_contrast},
    rgb::{color_at_depth, lerp_rgb, scale_channel},
};

#[derive(Debug, Clone, Copy)]
pub struct ActiveTheme<'a> {
    pub theme: &'a Theme,
    pub depth: ColorDepth,
    bars: BarColorOverrides,
}

impl<'a> ActiveTheme<'a> {
    #[must_use]
    pub fn new(theme: &'a Theme, depth: ColorDepth) -> Self {
        Self {
            theme,
            depth,
            bars: BarColorOverrides::default(),
        }
    }

    #[must_use]
    pub fn with_bars(self, bars: BarColorOverrides) -> Self {
        Self { bars, ..self }
    }

    #[must_use]
    pub fn volume_bar_colors(&self) -> BarColors {
        bar_colors(BarColorOverrides::default(), self.theme)
    }

    fn fill_colors(&self, bar: BarColors) -> FillColors {
        FillColors {
            accent: self.color(bar.fill),
            dim: self.color(bar.trough),
        }
    }

    #[must_use]
    pub fn progress_colors(&self) -> FillColors {
        self.fill_colors(bar_colors(self.bars, self.theme))
    }

    #[must_use]
    pub fn volume_fill_colors(&self) -> FillColors {
        self.fill_colors(self.volume_bar_colors())
    }

    #[must_use]
    pub fn color(&self, rgb: Rgb) -> Color {
        color_at_depth(rgb, self.depth)
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
    pub fn text(&self) -> Color {
        self.role(Role::Text)
    }

    #[must_use]
    pub fn accent(&self) -> Color {
        self.role(Role::Accent)
    }

    #[must_use]
    pub fn secondary_accent(&self) -> Color {
        self.role(Role::Accent2)
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
    pub fn dim(&self) -> Color {
        self.role(Role::Dim)
    }

    #[must_use]
    pub fn border(&self) -> Color {
        self.role(Role::Frame)
    }

    #[must_use]
    pub fn window_background(&self) -> Color {
        self.role(Role::WindowBackground)
    }

    #[must_use]
    pub fn background(&self) -> Color {
        self.role(Role::Background)
    }

    #[must_use]
    pub fn selection_foreground(&self) -> Color {
        self.role(Role::SelectionForeground)
    }

    #[must_use]
    pub fn selection_background(&self) -> Color {
        self.role(Role::SelectionBackground)
    }

    #[must_use]
    pub fn highlight(&self) -> Color {
        self.role(Role::Highlight)
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
    use ratatui::style::Color;

    use crate::theme::{
        ColorDepth,
        Role,
        Theme,
        active_theme::ActiveTheme,
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
}
