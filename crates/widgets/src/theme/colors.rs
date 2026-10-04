use std::fmt;

use kernel::domain::appearance::Rgb;
use strum::{EnumIter, IntoEnumIterator};

use crate::theme::{
    contrast::{
        MIN_MARKER_CONTRAST,
        MIN_SELECTION_TEXT_CONTRAST,
        raise_contrast,
        visible_band,
    },
    rgb::{gradient_at, lerp_rgb},
};

const WINDOW_BG_MIX: f32 = 0.06;
const SELECTION_BG_MIX: f32 = 0.18;
const BAR_GROOVE_MIX: f32 = 0.28;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum Role {
    Background,
    WindowBackground,
    Text,
    Accent,
    Accent2,
    SelectionForeground,
    SelectionBackground,
    Highlight,
    Frame,
    Dim,
    BarGroove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeBase {
    pub background: Rgb,
    pub foreground: Rgb,
    pub bright_foreground: Rgb,
    pub accent: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub red: Rgb,
    pub window_background: Option<Rgb>,
}

#[derive(Clone, PartialEq, Default)]
pub struct Colors {
    background: Rgb,
    window_background: Rgb,
    text: Rgb,
    accent: Rgb,
    accent2: Rgb,
    selection_foreground: Rgb,
    selection_background: Rgb,
    highlight: Rgb,
    frame: Rgb,
    dim: Rgb,
    bar_groove: Rgb,
    pub spectrum: [Rgb; 3],
}

impl fmt::Debug for Colors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let roles: Vec<Rgb> = Role::iter().map(|role| self.role(role)).collect();
        f.debug_struct("Colors")
            .field("roles", &roles)
            .field("spectrum", &self.spectrum)
            .finish()
    }
}

impl Colors {
    pub fn role(&self, role: Role) -> Rgb {
        match role {
            Role::Background => self.background,
            Role::WindowBackground => self.window_background,
            Role::Text => self.text,
            Role::Accent => self.accent,
            Role::Accent2 => self.accent2,
            Role::SelectionForeground => self.selection_foreground,
            Role::SelectionBackground => self.selection_background,
            Role::Highlight => self.highlight,
            Role::Frame => self.frame,
            Role::Dim => self.dim,
            Role::BarGroove => self.bar_groove,
        }
    }

    pub fn spectrum_color_at(&self, t: f32) -> Rgb {
        gradient_at(&self.spectrum, t).unwrap_or(self.spectrum[1])
    }

    #[must_use]
    pub fn derive(file: &ThemeBase) -> Colors {
        let window_bg = file.window_background.unwrap_or_else(|| {
            lerp_rgb(file.background, file.foreground, WINDOW_BG_MIX)
        });
        let selection_bg =
            visible_band(window_bg, file.bright_foreground, SELECTION_BG_MIX);
        Colors {
            background: file.background,
            window_background: window_bg,
            text: file.bright_foreground,
            accent: file.accent,
            accent2: file.yellow,
            selection_foreground: raise_contrast(
                file.bright_foreground,
                &[selection_bg],
                MIN_SELECTION_TEXT_CONTRAST,
            ),
            selection_background: selection_bg,
            highlight: raise_contrast(
                file.accent,
                &[window_bg, selection_bg],
                MIN_MARKER_CONTRAST,
            ),
            frame: file.foreground,
            dim: file.foreground,
            bar_groove: visible_band(window_bg, file.bright_foreground, BAR_GROOVE_MIX),
            spectrum: [file.green, file.yellow, file.red],
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;

    use crate::theme::{
        colors::{Colors, Role, ThemeBase},
        contrast::{
            MIN_BAND_CONTRAST,
            MIN_MARKER_CONTRAST,
            MIN_SELECTION_TEXT_CONTRAST,
            contrast_ratio,
        },
    };

    fn test_colors_file() -> ThemeBase {
        ThemeBase {
            background: Rgb([0x10, 0x20, 0x30]),
            foreground: Rgb([0x40, 0x50, 0x60]),
            bright_foreground: Rgb([0x70, 0x80, 0x90]),
            accent: Rgb([0xa0, 0xb0, 0xc0]),
            green: Rgb([0, 0xff, 0]),
            yellow: Rgb([0xff, 0xff, 0]),
            red: Rgb([0xff, 0, 0]),
            window_background: None,
        }
    }

    #[test]
    fn the_derivation_table_maps_every_role() {
        insta::assert_debug_snapshot!(Colors::derive(&test_colors_file()));
    }

    #[test]
    fn every_role_reads_back_the_hex_the_derivation_table_wrote() {
        let colors = Colors::derive(&test_colors_file());
        assert_eq!(colors.role(Role::Background), Rgb([0x10, 0x20, 0x30]));
        assert_eq!(colors.role(Role::Accent2), Rgb([0xff, 0xff, 0]));
    }

    #[test]
    fn a_theme_whose_accent_is_its_text_still_derives_a_visible_band() {
        let cream = Rgb([0xf3, 0xe9, 0xd2]);
        let file = ThemeBase {
            background: Rgb([0x0b, 0x0b, 0x0b]),
            foreground: Rgb([0x8f, 0x8a, 0x80]),
            bright_foreground: cream,
            accent: cream,
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        let window_bg = colors.role(Role::WindowBackground);
        let selection_bg = colors.role(Role::SelectionBackground);
        assert!(contrast_ratio(selection_bg, window_bg) >= MIN_BAND_CONTRAST);
        assert!(
            contrast_ratio(colors.role(Role::SelectionForeground), selection_bg)
                >= MIN_SELECTION_TEXT_CONTRAST
        );
        let highlight = colors.role(Role::Highlight);
        assert!(contrast_ratio(highlight, window_bg) >= MIN_MARKER_CONTRAST);
        assert!(contrast_ratio(highlight, selection_bg) >= MIN_MARKER_CONTRAST);
        assert!(
            contrast_ratio(colors.role(Role::BarGroove), window_bg)
                >= MIN_BAND_CONTRAST,
            "a bar's unfilled track has to be visible on the card it is painted on"
        );
    }

    fn luma(rgb: Rgb) -> u32 {
        rgb.0.iter().map(|&channel| u32::from(channel)).sum()
    }

    #[test]
    fn window_bg_lightens_toward_fg_on_a_dark_theme() {
        let file = ThemeBase {
            background: Rgb([0x10, 0x10, 0x10]),
            foreground: Rgb([0xe0, 0xe0, 0xe0]),
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        assert!(
            luma(colors.role(Role::WindowBackground))
                > luma(colors.role(Role::Background))
        );
    }

    #[test]
    fn window_bg_darkens_toward_fg_on_a_light_theme() {
        let file = ThemeBase {
            background: Rgb([0xe0, 0xe0, 0xe0]),
            foreground: Rgb([0x10, 0x10, 0x10]),
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        assert!(
            luma(colors.role(Role::WindowBackground))
                < luma(colors.role(Role::Background))
        );
    }
}
