use kernel::domain::appearance::Rgb;
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
    let ratio = |candidate: Rgb| {
        against
            .iter()
            .map(|&background| contrast_ratio(candidate, background))
            .fold(f32::INFINITY, f32::min)
    };
    if ratio(color) >= minimum {
        return color;
    }
    let brightest = against.iter().fold(0.0_f32, |brightest, &background| {
        brightest.max(relative_luminance(background))
    });
    let targets = if relative_luminance(color) >= brightest {
        [LIGHTEST, DARKEST]
    } else {
        [DARKEST, LIGHTEST]
    };
    targets
        .iter()
        .flat_map(|&target| {
            NUDGE_LADDER
                .iter()
                .map(move |&step| lerp_rgb(color, target, step))
        })
        .find(|&candidate| ratio(candidate) >= minimum)
        .unwrap_or_else(|| {
            if ratio(LIGHTEST) >= ratio(DARKEST) {
                LIGHTEST
            } else {
                DARKEST
            }
        })
}

pub(crate) fn visible_band(window_background: Rgb, text: Rgb, mix: f32) -> Rgb {
    let band = lerp_rgb(window_background, text, mix);
    if contrast_ratio(band, window_background) >= MIN_BAND_CONTRAST {
        return band;
    }
    NUDGE_LADDER
        .iter()
        .map(|&step| lerp_rgb(window_background, text, mix + (1.0 - mix) * step))
        .find(|&candidate| {
            contrast_ratio(candidate, window_background) >= MIN_BAND_CONTRAST
        })
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;
    use rstest::rstest;

    use crate::theme::contrast::{
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

    #[rstest]
    #[case::black_on_white(BLACK, WHITE, 21.0)]
    #[case::white_on_black(WHITE, BLACK, 21.0)]
    #[case::a_colour_against_itself(
        Rgb([0x2a, 0xa8, 0xa0]),
        Rgb([0x2a, 0xa8, 0xa0]),
        1.0
    )]
    fn contrast_ratio_spans_one_to_twenty_one(
        #[case] first: Rgb,
        #[case] second: Rgb,
        #[case] ratio: f32,
    ) {
        assert!((contrast_ratio(first, second) - ratio).abs() < 0.01);
    }

    #[rstest]
    #[case::a_light_colour_on_a_light_background(
        Rgb([0xf3, 0xe9, 0xd2]),
        Rgb([0xf5, 0xf1, 0xe8]),
        BLACK
    )]
    #[case::a_dark_colour(
        Rgb([0x2a, 0xa8, 0xa0]),
        Rgb([0xd8, 0xd0, 0xc8]),
        BLACK
    )]
    #[case::a_light_colour_on_a_mid_background(
        Rgb([0x90, 0x90, 0x90]),
        Rgb([0x80, 0x80, 0x80]),
        WHITE
    )]
    fn raise_contrast_pushes_a_colour_toward_black_or_white(
        #[case] color: Rgb,
        #[case] background: Rgb,
        #[case] target: Rgb,
    ) {
        let raised = raise_contrast(color, &[background], MIN_MARKER_CONTRAST);
        assert!(contrast_ratio(raised, target) < contrast_ratio(color, target));
        assert!(contrast_ratio(raised, background) >= MIN_MARKER_CONTRAST);
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
