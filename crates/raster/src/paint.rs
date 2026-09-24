use config::Hex;

#[must_use]
pub(crate) fn skia_color(hex: Hex) -> tiny_skia::Color {
    skia_color_with_alpha(hex, 255)
}

#[must_use]
pub(crate) fn skia_color_with_alpha(hex: Hex, alpha: u8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(hex.0[0], hex.0[1], hex.0[2], alpha)
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("pixel canvas {width}x{height} is empty or too large")]
    Canvas { width: u32, height: u32 },
}

#[cfg(test)]
mod tests {
    use config::Hex;
    use rstest::rstest;

    use crate::paint::{skia_color, skia_color_with_alpha};

    #[rstest]
    #[case::opaque(255)]
    #[case::half(128)]
    fn skia_color_with_alpha_carries_the_alpha_it_is_given(#[case] alpha: u8) {
        assert_eq!(
            skia_color_with_alpha(Hex([10, 20, 30]), alpha),
            tiny_skia::Color::from_rgba8(10, 20, 30, alpha)
        );
    }

    #[test]
    fn skia_color_is_fully_opaque() {
        assert_eq!(
            skia_color(Hex([10, 20, 30])),
            tiny_skia::Color::from_rgba8(10, 20, 30, 255)
        );
    }
}
