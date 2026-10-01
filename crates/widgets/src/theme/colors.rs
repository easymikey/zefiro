use config::{Rgb, ThemeColors};
use strum::{EnumCount, EnumIter};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCount, EnumIter)]
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Colors {
    roles: [Rgb; Role::COUNT],
    pub spectrum: [Rgb; 3],
}

impl Colors {
    pub fn role(&self, role: Role) -> Rgb {
        self.roles.get(role as usize).copied().unwrap_or_default()
    }

    pub fn spectrum_color_at(&self, t: f32) -> Rgb {
        gradient_at(&self.spectrum, t).unwrap_or(self.spectrum[1])
    }

    #[must_use]
    pub fn derive(file: &ThemeColors) -> Colors {
        let window_bg = file.window_background.unwrap_or_else(|| {
            lerp_rgb(file.background, file.foreground, WINDOW_BG_MIX)
        });
        let selection_bg =
            visible_band(window_bg, file.bright_foreground, SELECTION_BG_MIX);
        let table = [
            (Role::Background, file.background),
            (Role::WindowBackground, window_bg),
            (Role::Text, file.bright_foreground),
            (Role::Accent, file.accent),
            (Role::Accent2, file.yellow),
            (
                Role::SelectionForeground,
                raise_contrast(
                    file.bright_foreground,
                    &[selection_bg],
                    MIN_SELECTION_TEXT_CONTRAST,
                ),
            ),
            (Role::SelectionBackground, selection_bg),
            (
                Role::Highlight,
                raise_contrast(
                    file.accent,
                    &[window_bg, selection_bg],
                    MIN_MARKER_CONTRAST,
                ),
            ),
            (Role::Frame, file.foreground),
            (Role::Dim, file.foreground),
            (
                Role::BarGroove,
                visible_band(window_bg, file.bright_foreground, BAR_GROOVE_MIX),
            ),
        ];
        let mut roles = [Rgb::default(); Role::COUNT];
        for (role, rgb) in table {
            if let Some(slot) = roles.get_mut(role as usize) {
                *slot = rgb;
            }
        }
        Colors {
            roles,
            spectrum: [file.green, file.yellow, file.red],
        }
    }
}

#[cfg(test)]
mod tests {
    use config::{Rgb, ThemeColors};
    use strum::IntoEnumIterator;

    use crate::theme::{
        colors::{Colors, Role},
        contrast::{
            MIN_BAND_CONTRAST,
            MIN_MARKER_CONTRAST,
            MIN_SELECTION_TEXT_CONTRAST,
            contrast_ratio,
        },
    };

    fn test_colors_file() -> ThemeColors {
        ThemeColors {
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
        assert_eq!(Role::iter().count(), Colors::default().roles.len());
    }

    #[test]
    fn a_theme_whose_accent_is_its_text_still_derives_a_visible_band() {
        let cream = Rgb([0xf3, 0xe9, 0xd2]);
        let file = ThemeColors {
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
        let file = ThemeColors {
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
        let file = ThemeColors {
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
