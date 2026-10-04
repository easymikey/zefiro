use std::{path::PathBuf, sync::Arc};

use image::{RgbaImage, imageops::FilterType};
use kernel::domain::geometry::Pixels;
use ratatui::layout::Rect;

use crate::{
    pixels::{
        cover::CoverImage,
        vinyl::{VinylCache, VinylCacheKey, VinylStyle},
    },
    scene::Scene,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    Plain(PathBuf),
    Vinyl(VinylCacheKey),
}

impl Identity {
    pub(crate) fn changed_only_by_theme(&self, desired: &Self) -> bool {
        match (self, desired) {
            (Self::Vinyl(old), Self::Vinyl(new)) => {
                old.theme_revision != new.theme_revision
                    && old.config_revision == new.config_revision
                    && old.path == new.path
                    && old.size_px == new.size_px
            }
            (Self::Plain(_), _) | (Self::Vinyl(_), Self::Plain(_)) => false,
        }
    }
}

pub(crate) struct BuiltPixmap {
    pub pixmap: Arc<RgbaImage>,
    pub identity: Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPixels {
    pub width: Pixels,
    pub height: Pixels,
}

#[must_use]
pub(crate) fn plain_pixmap(decoded: Option<&CoverImage>) -> Option<BuiltPixmap> {
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
    let width = u32::from(rect.width).saturating_mul(cell.width.0).max(1);
    let height = u32::from(rect.height).saturating_mul(cell.height.0).max(1);
    if image.width() == width && image.height() == height {
        return image;
    }
    image::imageops::resize(&image, width, height, FilterType::Lanczos3)
}

#[must_use]
pub(crate) fn vinyl_size(rect: Rect, cell: CellPixels) -> Pixels {
    Pixels(u32::from(rect.height).saturating_mul(cell.height.0))
}

#[must_use]
pub(crate) fn vinyl_key(
    scene: &Scene<'_>,
    decoded: Option<&CoverImage>,
    size: Pixels,
) -> VinylCacheKey {
    VinylCacheKey {
        config_revision: scene.revisions.config,
        theme_revision: scene.revisions.theme,
        path: decoded.map(|cover| cover.path.clone()),
        size_px: size.0,
        colors: VinylStyle::from_theme(&scene.active_theme()),
    }
}

#[must_use]
pub(crate) fn compose_vinyl(
    cache: &mut VinylCache,
    key: VinylCacheKey,
    decoded: Option<&CoverImage>,
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
    use kernel::domain::geometry::Pixels;
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::pixels::cover::pixmap::{CellPixels, fit_to_rect};

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    #[rstest]
    #[case::a_compact_card(Rect::new(0, 0, 16, 8), CellPixels { width: Pixels(9), height: Pixels(18) })]
    #[case::a_wide_terminal_card(Rect::new(2, 3, 24, 12), CellPixels { width: Pixels(8), height: Pixels(16) })]
    #[case::a_tall_cell_font(Rect::new(0, 0, 30, 15), CellPixels { width: Pixels(10), height: Pixels(20) })]
    fn a_plain_cover_is_fit_to_exactly_the_cover_squares_own_pixel_size(
        #[case] rect: Rect,
        #[case] cell: CellPixels,
    ) {
        let fitted = fit_to_rect(source_pixmap(), rect, cell);
        assert_eq!(
            fitted.width(),
            u32::from(rect.width) * cell.width.0,
            "the fitted width must match the cover square converted to pixels"
        );
        assert_eq!(
            fitted.height(),
            u32::from(rect.height) * cell.height.0,
            "the fitted height must match the cover square converted to pixels"
        );
    }

    #[test]
    fn a_square_cover_cell_rect_stays_square_in_pixels() {
        let cell_aspect: u16 = 2;
        let cell = CellPixels {
            width: Pixels(9),
            height: Pixels(9 * u32::from(cell_aspect)),
        };
        let height = 8u16;
        let width = height * cell_aspect;
        let rect = Rect::new(0, 0, width, height);

        let fitted = fit_to_rect(source_pixmap(), rect, cell);
        assert_eq!(
            fitted.width(),
            fitted.height(),
            "a plain cover's cell rect built with cover_aspect 1.0 must render \
             as a square in pixels once fit to the target"
        );
    }
}
