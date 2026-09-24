use config::{Hex, ThemeColors};
use strum::{EnumCount, EnumIter};

use crate::theme::{
    contrast::{
        MIN_MARKER_CONTRAST,
        MIN_SELECTION_TEXT_CONTRAST,
        raise_contrast,
        visible_band,
    },
    hex::{lerp_rgb, palette_at},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCount, EnumIter)]
pub enum Role {
    Background,
    WindowBg,
    Text,
    Accent,
    Accent2,
    SelectionFg,
    SelectionBg,
    Highlight,
    Frame,
    Dim,
    BarGroove,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Colors {
    roles: [Hex; Role::COUNT],
    pub spectrum: [Hex; 3],
}

impl Colors {
    pub fn role(&self, role: Role) -> Hex {
        self.roles.get(role as usize).copied().unwrap_or_default()
    }

    pub fn spectrum_color_at(&self, t: f32) -> Hex {
        palette_at(&self.spectrum, t).unwrap_or(self.spectrum[1])
    }

    #[must_use]
    pub fn derive(file: &ThemeColors) -> Colors {
        Colors::derive_with(file, &ColorMix::default())
    }

    #[must_use]
    pub(crate) fn derive_with(file: &ThemeColors, tuning: &ColorMix) -> Colors {
        let window_bg = file.window_background.unwrap_or_else(|| {
            lerp_rgb(file.background, file.foreground, tuning.window_bg_mix)
        });
        let selection_bg =
            visible_band(window_bg, file.bright_foreground, tuning.selection_bg_mix);
        let table = [
            (Role::Background, file.background),
            (Role::WindowBg, window_bg),
            (Role::Text, file.bright_foreground),
            (Role::Accent, file.accent),
            (Role::Accent2, file.yellow),
            (
                Role::SelectionFg,
                raise_contrast(
                    file.bright_foreground,
                    &[selection_bg],
                    MIN_SELECTION_TEXT_CONTRAST,
                ),
            ),
            (Role::SelectionBg, selection_bg),
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
                visible_band(window_bg, file.bright_foreground, tuning.bar_groove_mix),
            ),
        ];
        let mut roles = [Hex::default(); Role::COUNT];
        for (role, hex) in table {
            if let Some(slot) = roles.get_mut(role as usize) {
                *slot = hex;
            }
        }
        Colors {
            roles,
            spectrum: [file.green, file.yellow, file.red],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ColorMix {
    pub(crate) window_bg_mix: f32,
    pub(crate) selection_bg_mix: f32,
    pub(crate) bar_groove_mix: f32,
}

impl Default for ColorMix {
    fn default() -> Self {
        Self {
            window_bg_mix: 0.06,
            selection_bg_mix: 0.18,
            bar_groove_mix: 0.28,
        }
    }
}

impl From<ThemeColors> for Colors {
    fn from(file: ThemeColors) -> Self {
        Colors::derive(&file)
    }
}

#[cfg(test)]
mod tests {
    use config::{Hex, ThemeColors};
    use strum::IntoEnumIterator;

    use crate::theme::{
        contrast::{
            MIN_BAND_CONTRAST,
            MIN_MARKER_CONTRAST,
            MIN_SELECTION_TEXT_CONTRAST,
            contrast_ratio,
        },
        palette::{Colors, Role},
    };

    fn test_colors_file() -> ThemeColors {
        ThemeColors {
            background: Hex([0x10, 0x20, 0x30]),
            foreground: Hex([0x40, 0x50, 0x60]),
            bright_foreground: Hex([0x70, 0x80, 0x90]),
            accent: Hex([0xa0, 0xb0, 0xc0]),
            green: Hex([0, 0xff, 0]),
            yellow: Hex([0xff, 0xff, 0]),
            red: Hex([0xff, 0, 0]),
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
        assert_eq!(colors.role(Role::Background), Hex([0x10, 0x20, 0x30]));
        assert_eq!(colors.role(Role::Accent2), Hex([0xff, 0xff, 0]));
        assert_eq!(Role::iter().count(), Colors::default().roles.len());
    }

    #[test]
    fn a_theme_whose_accent_is_its_text_still_derives_a_visible_band() {
        let cream = Hex([0xf3, 0xe9, 0xd2]);
        let file = ThemeColors {
            background: Hex([0x0b, 0x0b, 0x0b]),
            foreground: Hex([0x8f, 0x8a, 0x80]),
            bright_foreground: cream,
            accent: cream,
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        let window_bg = colors.role(Role::WindowBg);
        let selection_bg = colors.role(Role::SelectionBg);
        assert!(contrast_ratio(selection_bg, window_bg) >= MIN_BAND_CONTRAST);
        assert!(
            contrast_ratio(colors.role(Role::SelectionFg), selection_bg)
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

    fn luma(hex: Hex) -> u32 {
        hex.0.iter().map(|&channel| u32::from(channel)).sum()
    }

    #[test]
    fn window_bg_lightens_toward_fg_on_a_dark_theme() {
        let file = ThemeColors {
            background: Hex([0x10, 0x10, 0x10]),
            foreground: Hex([0xe0, 0xe0, 0xe0]),
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        assert!(
            luma(colors.role(Role::WindowBg)) > luma(colors.role(Role::Background))
        );
    }

    #[test]
    fn window_bg_darkens_toward_fg_on_a_light_theme() {
        let file = ThemeColors {
            background: Hex([0xe0, 0xe0, 0xe0]),
            foreground: Hex([0x10, 0x10, 0x10]),
            ..test_colors_file()
        };
        let colors = Colors::derive(&file);
        assert!(
            luma(colors.role(Role::WindowBg)) < luma(colors.role(Role::Background))
        );
    }
}
