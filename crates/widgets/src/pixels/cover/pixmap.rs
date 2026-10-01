use std::sync::Arc;

use image::{RgbaImage, imageops::FilterType};
use ratatui::layout::Rect;

use crate::{
    DecodedCover,
    Scene,
    VinylCache,
    VinylCacheKey,
    VinylStyle,
    pixels::cover::lifecycle::{BuiltPixmap, Identity},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPixels {
    pub width: u16,
    pub height: u16,
}

#[must_use]
pub(crate) fn plain_pixmap(decoded: Option<&DecodedCover>) -> Option<BuiltPixmap> {
    let decoded = decoded?;
    Some(BuiltPixmap {
        pixmap: Arc::clone(&decoded.image),
        identity: Identity::Plain(decoded.path.clone()),
    })
}

#[must_use]
pub(crate) fn translucent(image: &RgbaImage) -> bool {
    image.pixels().any(|pixel| pixel.0[3] < 255)
}

#[must_use]
pub(crate) fn fit_to_rect(image: RgbaImage, rect: Rect, cell: CellPixels) -> RgbaImage {
    let width = u32::from(rect.width)
        .saturating_mul(u32::from(cell.width))
        .max(1);
    let height = u32::from(rect.height)
        .saturating_mul(u32::from(cell.height))
        .max(1);
    if image.width() == width && image.height() == height {
        return image;
    }
    image::imageops::resize(&image, width, height, FilterType::Lanczos3)
}

#[must_use]
pub(crate) fn vinyl_size_px(rect: Rect, cell: CellPixels) -> u32 {
    u32::from(rect.height).saturating_mul(u32::from(cell.height))
}

#[must_use]
pub(crate) fn vinyl_key(
    scene: &Scene<'_>,
    decoded: Option<&DecodedCover>,
    size_px: u32,
) -> VinylCacheKey {
    VinylCacheKey {
        config_revision: scene.model.revisions.config,
        theme_revision: scene.model.revisions.theme,
        path: decoded.map(|cover| cover.path.clone()),
        size_px,
        colors: VinylStyle::from_theme(&scene.active_theme()),
    }
}

#[must_use]
pub(crate) fn compose_vinyl(
    cache: &mut VinylCache,
    key: VinylCacheKey,
    decoded: Option<&DecodedCover>,
) -> BuiltPixmap {
    let art = decoded.map(|cover| cover.image.as_ref());
    let pixmap = Arc::new(cache.compose(key.clone(), art).clone());
    BuiltPixmap {
        pixmap,
        identity: Identity::Vinyl(key),
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::pixels::cover::pixmap::{CellPixels, fit_to_rect};

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    #[rstest]
    #[case::a_compact_card(Rect::new(0, 0, 16, 8), CellPixels { width: 9, height: 18 })]
    #[case::a_wide_terminal_card(Rect::new(2, 3, 24, 12), CellPixels { width: 8, height: 16 })]
    #[case::a_tall_cell_font(Rect::new(0, 0, 30, 15), CellPixels { width: 10, height: 20 })]
    fn a_plain_cover_is_fit_to_exactly_the_cover_squares_own_pixel_size(
        #[case] rect: Rect,
        #[case] cell: CellPixels,
    ) {
        let fitted = fit_to_rect(source_pixmap(), rect, cell);
        assert_eq!(
            fitted.width(),
            u32::from(rect.width) * u32::from(cell.width),
            "the fitted width must match the cover square converted to pixels"
        );
        assert_eq!(
            fitted.height(),
            u32::from(rect.height) * u32::from(cell.height),
            "the fitted height must match the cover square converted to pixels"
        );
    }

    #[test]
    fn a_square_cover_cell_rect_stays_square_in_pixels() {
        let cell_aspect: u16 = 2;
        let cell = CellPixels {
            width: 9,
            height: 9 * cell_aspect,
        };
        let height_cells = 8u16;
        let width_cells = height_cells * cell_aspect;
        let rect = Rect::new(0, 0, width_cells, height_cells);

        let fitted = fit_to_rect(source_pixmap(), rect, cell);
        assert_eq!(
            fitted.width(),
            fitted.height(),
            "a plain cover's cell rect built with cover_aspect 1.0 must render \
             as a square in pixels once fit to the target"
        );
    }
}
