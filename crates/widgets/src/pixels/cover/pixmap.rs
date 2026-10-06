use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fast_image_resize::CropBox;
use image::RgbaImage;
use kernel::domain::geometry::Pixels;
use ratatui::layout::Rect;

use crate::pixels::{
    cover::CoverImage,
    vinyl::{
        VinylCache,
        VinylCacheKey,
        VinylStyle,
        art::{ResampleError, resample},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    Plain(PathBuf),
    Vinyl(VinylCacheKey),
}

impl Identity {
    pub(crate) fn is(&self, wanted: &Wanted<'_>) -> bool {
        match self {
            Self::Plain(path) => wanted.path() == Some(path.as_path()),
            Self::Vinyl(key) => {
                key.path.as_deref() == wanted.path()
                    && key.size == wanted.pixels
                    && key.colors == wanted.vinyl_style
            }
        }
    }

    pub(crate) fn changed_only_by_theme(&self, wanted: &Wanted<'_>) -> bool {
        match self {
            Self::Vinyl(key) => {
                key.colors != wanted.vinyl_style
                    && key.path.as_deref() == wanted.path()
                    && key.size == wanted.pixels
            }
            Self::Plain(_) => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Wanted<'a> {
    pub cover_image: Option<&'a CoverImage>,
    pub pixels: Pixels,
    pub vinyl_style: VinylStyle,
}

impl Wanted<'_> {
    fn path(&self) -> Option<&Path> {
        self.cover_image.map(|cover| cover.path.as_path())
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
pub(crate) fn fit_to_rect(
    image: Arc<RgbaImage>,
    rect: Rect,
    cell: CellPixels,
) -> Arc<RgbaImage> {
    let width = u32::from(rect.width).saturating_mul(cell.width.0).max(1);
    let height = u32::from(rect.height).saturating_mul(cell.height.0).max(1);
    if image.dimensions() == (width, height) {
        return image;
    }
    let whole = CropBox {
        left: 0.0,
        top: 0.0,
        width: f64::from(image.width()),
        height: f64::from(image.height()),
    };
    match resample(&image, whole, (width, height)) {
        Ok(fitted) => Arc::new(fitted),
        Err(
            ResampleError::SourceBuffer(_)
            | ResampleError::Resize(_)
            | ResampleError::TargetBuffer { .. },
        ) => image,
    }
}

#[must_use]
pub(crate) fn vinyl_size(rect: Rect, cell: CellPixels) -> Pixels {
    Pixels(u32::from(rect.height).saturating_mul(cell.height.0))
}

#[must_use]
pub(crate) fn vinyl_key(wanted: &Wanted<'_>) -> VinylCacheKey {
    VinylCacheKey {
        path: wanted.path().map(Path::to_path_buf),
        size: wanted.pixels,
        colors: wanted.vinyl_style,
    }
}

#[must_use]
pub(crate) fn compose_vinyl(
    cache: &mut VinylCache,
    key: &VinylCacheKey,
    decoded: Option<&CoverImage>,
) -> Arc<RgbaImage> {
    let art = decoded.map(|cover| cover.image.as_ref());
    Arc::new(cache.compose(key, art))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::{Rgba, RgbaImage};
    use kernel::domain::geometry::Pixels;
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::pixels::cover::pixmap::{CellPixels, fit_to_rect};

    fn source_pixmap() -> Arc<RgbaImage> {
        Arc::new(RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255])))
    }

    #[test]
    fn an_already_fitted_pixmap_is_shared_instead_of_resized() {
        let rect = Rect::new(0, 0, 2, 1);
        let cell_pixels = CellPixels {
            width: Pixels(4),
            height: Pixels(8),
        };
        let fitted = fit_to_rect(source_pixmap(), rect, cell_pixels);
        let refitted = fit_to_rect(Arc::clone(&fitted), rect, cell_pixels);
        assert_eq!(fitted.dimensions(), (8, 8));
        assert!(Arc::ptr_eq(&fitted, &refitted));
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
