use config::Rgb;
use palette::{Srgb, color_difference::Wcag21RelativeContrast};

use crate::theme::rgb::lerp_rgb;

pub(crate) const MIN_BAND_CONTRAST: f32 = 1.5;
pub(crate) const MIN_SELECTION_TEXT_CONTRAST: f32 = 4.5;
pub(crate) const MIN_MARKER_CONTRAST: f32 = 3.0;

const DARKEST: Rgb = Rgb([u8::MIN, u8::MIN, u8::MIN]);
const LIGHTEST: Rgb = Rgb([u8::MAX, u8::MAX, u8::MAX]);
const NUDGE_LADDER: [f32; 10] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

fn srgb(color: Rgb) -> Srgb<f32> {
    let [red, green, blue] = color.0;
    Srgb::new(red, green, blue).into_format()
}

#[must_use]
pub(crate) fn relative_luminance(color: Rgb) -> f32 {
    srgb(color).relative_luminance().luma
}

#[must_use]
pub(crate) fn contrast_ratio(first: Rgb, second: Rgb) -> f32 {
    srgb(first).relative_contrast(srgb(second))
}

pub(crate) fn raise_contrast(color: Rgb, against: &[Rgb], minimum: f32) -> Rgb {
    let clears = |candidate: Rgb| {
        against
            .iter()
            .all(|&background| contrast_ratio(candidate, background) >= minimum)
    };
    if clears(color) {
        return color;
    }
    let brightest = against.iter().fold(0.0_f32, |brightest, &background| {
        brightest.max(relative_luminance(background))
    });
    let target = if relative_luminance(color) >= brightest {
        LIGHTEST
    } else {
        DARKEST
    };
    NUDGE_LADDER
        .iter()
        .map(|&step| lerp_rgb(color, target, step))
        .find(|&candidate| clears(candidate))
        .unwrap_or(target)
}

pub(crate) fn visible_band(window_bg: Rgb, text: Rgb, mix: f32) -> Rgb {
    let band = lerp_rgb(window_bg, text, mix);
    if contrast_ratio(band, window_bg) >= MIN_BAND_CONTRAST {
        return band;
    }
    NUDGE_LADDER
        .iter()
        .map(|&step| lerp_rgb(window_bg, text, mix + (1.0 - mix) * step))
        .find(|&candidate| contrast_ratio(candidate, window_bg) >= MIN_BAND_CONTRAST)
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use config::Rgb;

    use crate::theme::contrast::{
        MIN_BAND_CONTRAST,
        MIN_MARKER_CONTRAST,
        contrast_ratio,
        raise_contrast,
        relative_luminance,
        visible_band,
    };

    const BLACK: Rgb = Rgb([0, 0, 0]);
    const WHITE: Rgb = Rgb([0xff, 0xff, 0xff]);

    #[test]
    fn luminance_spans_black_to_white() {
        assert!(relative_luminance(BLACK).abs() < f32::EPSILON);
        assert!((relative_luminance(WHITE) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn black_on_white_is_the_maximum_ratio() {
        assert!((contrast_ratio(BLACK, WHITE) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(WHITE, BLACK) - 21.0).abs() < 0.01);
    }

    #[test]
    fn a_colour_against_itself_is_one_to_one() {
        assert!(
            (contrast_ratio(Rgb([0x2a, 0xa8, 0xa0]), Rgb([0x2a, 0xa8, 0xa0])) - 1.0)
                .abs()
                < 0.01
        );
    }

    #[test]
    fn raise_contrast_leaves_a_colour_that_already_clears_the_bar_alone() {
        assert_eq!(raise_contrast(WHITE, &[BLACK], MIN_MARKER_CONTRAST), WHITE);
    }

    #[test]
    fn raise_contrast_pushes_a_light_colour_further_from_a_light_background() {
        let cream = Rgb([0xf3, 0xe9, 0xd2]);
        let raised =
            raise_contrast(cream, &[Rgb([0xf5, 0xf1, 0xe8])], MIN_MARKER_CONTRAST);
        assert_ne!(raised, cream);
        assert!(contrast_ratio(raised, Rgb([0xf5, 0xf1, 0xe8])) >= MIN_MARKER_CONTRAST);
    }

    #[test]
    fn raise_contrast_pushes_a_dark_colour_toward_black() {
        let teal = Rgb([0x2a, 0xa8, 0xa0]);
        let paper = Rgb([0xd8, 0xd0, 0xc8]);
        let raised = raise_contrast(teal, &[paper], MIN_MARKER_CONTRAST);
        assert!(relative_luminance(raised) < relative_luminance(teal));
        assert!(contrast_ratio(raised, paper) >= MIN_MARKER_CONTRAST);
    }

    #[test]
    fn a_band_clears_the_band_ratio_even_when_the_mix_alone_would_not() {
        let window_bg = Rgb([0x0b, 0x0b, 0x0b]);
        let text = Rgb([0xf5, 0xf1, 0xe8]);
        let band = visible_band(window_bg, text, 0.0);
        assert!(contrast_ratio(band, window_bg) >= MIN_BAND_CONTRAST);
    }

    #[test]
    fn a_band_lightens_a_dark_theme_and_darkens_a_light_one() {
        let dark = Rgb([0x12, 0x12, 0x11]);
        let light = Rgb([0xf3, 0xe9, 0xc1]);
        assert!(
            relative_luminance(visible_band(dark, WHITE, 0.18))
                > relative_luminance(dark)
        );
        assert!(
            relative_luminance(visible_band(light, BLACK, 0.18))
                < relative_luminance(light)
        );
    }
}
