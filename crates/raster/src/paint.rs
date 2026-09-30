use config::Rgb;

#[must_use]
pub(crate) fn skia_color(rgb: Rgb) -> tiny_skia::Color {
    skia_color_with_alpha(rgb, 255)
}

#[must_use]
pub(crate) fn skia_color_with_alpha(rgb: Rgb, alpha: u8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(rgb.0[0], rgb.0[1], rgb.0[2], alpha)
}

#[cfg(test)]
mod tests {
    use config::Rgb;
    use rstest::rstest;

    use crate::paint::{skia_color, skia_color_with_alpha};

    #[rstest]
    #[case::opaque(255)]
    #[case::half(128)]
    fn skia_color_with_alpha_carries_the_alpha_it_is_given(#[case] alpha: u8) {
        assert_eq!(
            skia_color_with_alpha(Rgb([10, 20, 30]), alpha),
            tiny_skia::Color::from_rgba8(10, 20, 30, alpha)
        );
    }

    #[test]
    fn skia_color_is_fully_opaque() {
        assert_eq!(
            skia_color(Rgb([10, 20, 30])),
            tiny_skia::Color::from_rgba8(10, 20, 30, 255)
        );
    }
}
