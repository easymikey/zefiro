use std::sync::Arc;

use image::{RgbaImage, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::FontSize;

use crate::pixels::cover::{
    DecodedCover,
    lifecycle::{Built, Identity},
};

#[must_use]
pub(crate) fn plain_pixmap(decoded: Option<&DecodedCover>) -> Option<Built> {
    let decoded = decoded?;
    Some(Built {
        pixmap: Arc::clone(&decoded.image),
        identity: Identity::Plain(decoded.path.clone()),
    })
}

#[must_use]
pub(crate) fn translucent(image: &RgbaImage) -> bool {
    image.pixels().any(|pixel| pixel.0[3] < 255)
}

/// Resizes the plain cover's pixmap to exactly fill `rect` in pixels, so the
/// placed image never depends on the resize protocol's own fit heuristics
/// for a source resolution that may differ from the decoded cover's size.
#[must_use]
pub(crate) fn fit_to_rect(
    image: RgbaImage,
    rect: Rect,
    font_size: FontSize,
) -> RgbaImage {
    let width = u32::from(rect.width)
        .saturating_mul(u32::from(font_size.width))
        .max(1);
    let height = u32::from(rect.height)
        .saturating_mul(u32::from(font_size.height))
        .max(1);
    if image.width() == width && image.height() == height {
        return image;
    }
    image::imageops::resize(&image, width, height, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use ratatui::layout::Rect;
    use ratatui_image::FontSize;
    use rstest::rstest;

    use crate::pixels::cover::pixel::fit_to_rect;

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    #[rstest]
    #[case::a_compact_card(Rect::new(0, 0, 16, 8), FontSize { width: 9, height: 18 })]
    #[case::a_wide_terminal_card(Rect::new(2, 3, 24, 12), FontSize { width: 8, height: 16 })]
    #[case::a_tall_cell_font(Rect::new(0, 0, 30, 15), FontSize { width: 10, height: 20 })]
    fn a_plain_cover_is_fit_to_exactly_the_cover_squares_own_pixel_size(
        #[case] rect: Rect,
        #[case] font_size: FontSize,
    ) {
        let fitted = fit_to_rect(source_pixmap(), rect, font_size);
        assert_eq!(
            fitted.width(),
            u32::from(rect.width) * u32::from(font_size.width),
            "the fitted width must match the cover square converted to pixels"
        );
        assert_eq!(
            fitted.height(),
            u32::from(rect.height) * u32::from(font_size.height),
            "the fitted height must match the cover square converted to pixels"
        );
    }

    #[test]
    fn a_square_cover_cell_rect_stays_square_in_pixels() {
        let cell_aspect: u16 = 2;
        let font_size = FontSize {
            width: 9,
            height: 9 * cell_aspect,
        };
        let height_cells = 8u16;
        let width_cells = height_cells * cell_aspect;
        let rect = Rect::new(0, 0, width_cells, height_cells);

        let fitted = fit_to_rect(source_pixmap(), rect, font_size);
        assert_eq!(
            fitted.width(),
            fitted.height(),
            "a plain cover's cell rect built with cover_aspect 1.0 must render \
             as a square in pixels once fit to the target"
        );
    }
}
