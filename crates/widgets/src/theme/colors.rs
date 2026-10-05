use kernel::domain::appearance::Rgb;

use crate::theme::{
    contrast::{
        MIN_MARKER_CONTRAST,
        MIN_SELECTION_TEXT_CONTRAST,
        raise_contrast,
        visible_band,
    },
    rgb::{gradient_at, lerp_rgb},
};

const WINDOW_BACKGROUND_MIX: f32 = 0.06;
const SELECTION_BACKGROUND_MIX: f32 = 0.18;
const BAR_GROOVE_MIX: f32 = 0.28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeBase {
    pub background: Rgb,
    pub muted_foreground: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub red: Rgb,
    pub window_background: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Colors<C = Rgb> {
    pub(crate) background: C,
    pub window_background: C,
    pub text: C,
    pub(crate) accent: C,
    pub(crate) accent2: C,
    pub(crate) selection_foreground: C,
    pub(crate) selection_background: C,
    pub(crate) highlight: C,
    pub(crate) muted_foreground: C,
    pub(crate) bar_groove: C,
    pub(crate) spectrum: [C; 3],
}

impl<C: Copy> Colors<C> {
    #[must_use]
    pub(crate) fn map<D>(&self, resolve: impl Fn(C) -> D) -> Colors<D> {
        Colors {
            background: resolve(self.background),
            window_background: resolve(self.window_background),
            text: resolve(self.text),
            accent: resolve(self.accent),
            accent2: resolve(self.accent2),
            selection_foreground: resolve(self.selection_foreground),
            selection_background: resolve(self.selection_background),
            highlight: resolve(self.highlight),
            muted_foreground: resolve(self.muted_foreground),
            bar_groove: resolve(self.bar_groove),
            spectrum: self.spectrum.map(resolve),
        }
    }
}

impl Colors {
    pub fn spectrum_color_at(&self, t: f32) -> Rgb {
        gradient_at(&self.spectrum, t).unwrap_or(self.spectrum[1])
    }

    #[must_use]
    pub fn derive(base: &ThemeBase) -> Colors {
        let window_background = base.window_background.unwrap_or_else(|| {
            lerp_rgb(
                base.background,
                base.muted_foreground,
                WINDOW_BACKGROUND_MIX,
            )
        });
        let selection_background =
            visible_band(window_background, base.foreground, SELECTION_BACKGROUND_MIX);
        Colors {
            background: base.background,
            window_background,
            text: base.foreground,
            accent: base.accent,
            accent2: base.yellow,
            selection_foreground: raise_contrast(
                base.foreground,
                &[selection_background],
                MIN_SELECTION_TEXT_CONTRAST,
            ),
            selection_background,
            highlight: raise_contrast(
                base.accent,
                &[window_background, selection_background],
                MIN_MARKER_CONTRAST,
            ),
            muted_foreground: base.muted_foreground,
            bar_groove: visible_band(
                window_background,
                base.foreground,
                BAR_GROOVE_MIX,
            ),
            spectrum: [base.green, base.yellow, base.red],
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;

    use crate::theme::{
        colors::{Colors, ThemeBase},
        contrast::{
            MIN_BAND_CONTRAST,
            MIN_MARKER_CONTRAST,
            MIN_SELECTION_TEXT_CONTRAST,
            contrast_ratio,
        },
    };

    fn test_base() -> ThemeBase {
        ThemeBase {
            background: Rgb([0x10, 0x20, 0x30]),
            muted_foreground: Rgb([0x40, 0x50, 0x60]),
            foreground: Rgb([0x70, 0x80, 0x90]),
            accent: Rgb([0xa0, 0xb0, 0xc0]),
            green: Rgb([0, 0xff, 0]),
            yellow: Rgb([0xff, 0xff, 0]),
            red: Rgb([0xff, 0, 0]),
            window_background: None,
        }
    }

    #[test]
    fn the_derivation_table_maps_every_role() {
        insta::assert_debug_snapshot!(Colors::derive(&test_base()));
    }

    #[test]
    fn every_field_reads_back_the_hex_the_derivation_table_wrote() {
        let colors = Colors::derive(&test_base());
        assert_eq!(colors.background, Rgb([0x10, 0x20, 0x30]));
        assert_eq!(colors.accent2, Rgb([0xff, 0xff, 0]));
    }

    #[test]
    fn a_theme_whose_accent_is_its_text_still_derives_a_visible_band() {
        let cream = Rgb([0xf3, 0xe9, 0xd2]);
        let base = ThemeBase {
            background: Rgb([0x0b, 0x0b, 0x0b]),
            muted_foreground: Rgb([0x8f, 0x8a, 0x80]),
            foreground: cream,
            accent: cream,
            ..test_base()
        };
        let colors = Colors::derive(&base);
        let window_background = colors.window_background;
        let selection_background = colors.selection_background;
        assert!(
            contrast_ratio(selection_background, window_background)
                >= MIN_BAND_CONTRAST
        );
        assert!(
            contrast_ratio(colors.selection_foreground, selection_background)
                >= MIN_SELECTION_TEXT_CONTRAST
        );
        let highlight = colors.highlight;
        assert!(contrast_ratio(highlight, window_background) >= MIN_MARKER_CONTRAST);
        assert!(contrast_ratio(highlight, selection_background) >= MIN_MARKER_CONTRAST);
        assert!(
            contrast_ratio(colors.bar_groove, window_background) >= MIN_BAND_CONTRAST,
            "a bar's unfilled track has to be visible on the card it is painted on"
        );
    }

    fn luma(rgb: Rgb) -> u32 {
        rgb.0.iter().map(|&channel| u32::from(channel)).sum()
    }

    #[test]
    fn window_background_lightens_toward_muted_foreground_on_a_dark_theme() {
        let base = ThemeBase {
            background: Rgb([0x10, 0x10, 0x10]),
            muted_foreground: Rgb([0xe0, 0xe0, 0xe0]),
            ..test_base()
        };
        let colors = Colors::derive(&base);
        assert!(luma(colors.window_background) > luma(colors.background));
    }

    #[test]
    fn window_background_darkens_toward_muted_foreground_on_a_light_theme() {
        let base = ThemeBase {
            background: Rgb([0xe0, 0xe0, 0xe0]),
            muted_foreground: Rgb([0x10, 0x10, 0x10]),
            ..test_base()
        };
        let colors = Colors::derive(&base);
        assert!(luma(colors.window_background) < luma(colors.background));
    }
}
