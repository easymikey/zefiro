use std::ops::Deref;

use config::Hex;
use raster::{BarColorOverrides, BarColors};
use ratatui::style::Color;

use crate::theme::{
    ColorDepth,
    Role,
    Theme,
    bars::{FillColors, bar_colors},
    contrast::{MIN_MARKER_CONTRAST, raise_contrast},
    hex::{color_at_depth, lerp_rgb, scale_channel},
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
    pub fn volume_bar(&self) -> BarColors {
        bar_colors(BarColorOverrides::default(), self.theme)
    }

    fn fill_colors(&self, bar: BarColors) -> FillColors {
        FillColors {
            accent: self.color(bar.fill),
            dim: self.color(bar.track),
        }
    }

    #[must_use]
    pub fn progress_colors(&self) -> FillColors {
        self.fill_colors(bar_colors(self.bars, self.theme))
    }

    #[must_use]
    pub fn volume_colors(&self) -> FillColors {
        self.fill_colors(self.volume_bar())
    }

    #[must_use]
    pub fn color(&self, hex: Hex) -> Color {
        color_at_depth(hex, self.depth)
    }

    #[must_use]
    pub fn lifted(&self, hex: Hex, toward_text: f32) -> Color {
        self.color(lerp_rgb(hex, self.colors.role(Role::Text), toward_text))
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
    pub fn accent2(&self) -> Color {
        self.role(Role::Accent2)
    }

    #[must_use]
    pub fn muted_accent(&self) -> Color {
        let accent = self.colors.role(Role::Accent).0;
        self.color(Hex(accent.map(|channel| scale_channel(channel, 0.82))))
    }

    #[must_use]
    pub fn favorite(&self) -> Color {
        self.color(raise_contrast(
            self.colors.role(Role::Accent2),
            &[
                self.colors.role(Role::WindowBg),
                self.colors.role(Role::SelectionBg),
            ],
            MIN_MARKER_CONTRAST,
        ))
    }

    #[must_use]
    pub fn dim(&self) -> Color {
        self.role(Role::Dim)
    }

    #[must_use]
    pub fn frame(&self) -> Color {
        self.role(Role::Frame)
    }

    #[must_use]
    pub fn window_bg(&self) -> Color {
        self.role(Role::WindowBg)
    }

    #[must_use]
    pub fn background(&self) -> Color {
        self.role(Role::Background)
    }

    #[must_use]
    pub fn selection_fg(&self) -> Color {
        self.role(Role::SelectionFg)
    }

    #[must_use]
    pub fn selection_bg(&self) -> Color {
        self.role(Role::SelectionBg)
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
