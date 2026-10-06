use image::RgbaImage;
use kernel::domain::geometry::Pixels;
use tiny_skia::{FillRule, IntSize, Mask, Path, Pixmap, PixmapPaint, Transform};

use crate::pixels::{
    numeric::{dimension_f32, floor},
    resample::cover_crop_resize,
    vinyl::geometry::{
        Disc,
        RoundedRect,
        VINYL_LAYOUT,
        VinylGeometry,
        circle_path,
        rounded_rect_path,
    },
};

#[must_use]
pub(crate) fn sleeve_inset_side(canvas_side: Pixels) -> u32 {
    let size = dimension_f32(canvas_side.0.max(1));
    let pad = VINYL_LAYOUT.sleeve_padding * size;
    floor::<u32>((size - pad * 2.0).round()).max(1)
}

fn label_diameter(canvas_side: Pixels) -> u32 {
    floor::<u32>((VinylGeometry::new(canvas_side).label_radius * 2.0).round()).max(1)
}

#[derive(Debug)]
pub(crate) struct VinylArt {
    pub(crate) sleeve: RgbaImage,
    pub(crate) label: RgbaImage,
}

#[must_use]
pub(crate) fn prepare_art(image: &RgbaImage, canvas_side: Pixels) -> VinylArt {
    let sleeve_side = sleeve_inset_side(canvas_side);
    let label_side = label_diameter(canvas_side);
    VinylArt {
        sleeve: cover_crop_resize(image, sleeve_side, sleeve_side),
        label: cover_crop_resize(image, label_side, label_side),
    }
}

pub(crate) struct ArtClip {
    path: Path,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl ArtClip {
    pub(crate) fn rounded_rect(rect: RoundedRect) -> Option<Self> {
        Some(Self {
            path: rounded_rect_path(rect)?,
            x: rect.x.round(),
            y: rect.y.round(),
            width: rect.width,
            height: rect.height,
        })
    }

    pub(crate) fn circle(disc: Disc) -> Option<Self> {
        let diameter = dimension_f32(floor::<u32>((disc.radius * 2.0).round()).max(1));
        Some(Self {
            path: circle_path(disc)?,
            x: disc.center_x - diameter / 2.0,
            y: disc.center_y - diameter / 2.0,
            width: diameter,
            height: diameter,
        })
    }
}

pub(crate) fn paint_art_clipped(pixmap: &mut Pixmap, art: &RgbaImage, clip: &ArtClip) {
    let target_width = floor::<u32>(clip.width.round()).max(1);
    let target_height = floor::<u32>(clip.height.round()).max(1);
    let sized = sized_or_resized(art, target_width, target_height);
    let Some(source) = pixmap_from(&sized) else {
        return;
    };
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    mask.fill_path(&clip.path, FillRule::Winding, true, Transform::identity());
    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint::default(),
        Transform::from_translate(clip.x, clip.y),
        Some(&mask),
    );
}

fn sized_or_resized(
    image: &RgbaImage,
    target_width: u32,
    target_height: u32,
) -> std::borrow::Cow<'_, RgbaImage> {
    if image.dimensions() == (target_width, target_height) {
        std::borrow::Cow::Borrowed(image)
    } else {
        std::borrow::Cow::Owned(cover_crop_resize(image, target_width, target_height))
    }
}

fn pixmap_from(image: &RgbaImage) -> Option<Pixmap> {
    let (width, height) = image.dimensions();
    let size = IntSize::from_wh(width, height)?;
    Pixmap::from_vec(image.as_raw().clone(), size)
}

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Pixels;

    use crate::pixels::{
        numeric::{dimension_f32, floor},
        vinyl::{
            VinylCache,
            VinylCacheKey,
            VinylStyle,
            art::{label_diameter, prepare_art, sleeve_inset_side},
            geometry::{VINYL_LAYOUT, shadow_horizontal_reach_fraction},
            test_support::synthetic_art,
        },
    };

    fn expected_peek(canvas_side: Pixels) -> u32 {
        let size = dimension_f32(canvas_side.0);
        let disc_diameter = VINYL_LAYOUT.disc_fraction * size;
        let peek = VINYL_LAYOUT.slide_fraction * disc_diameter;
        let shadow_margin =
            VINYL_LAYOUT.shadow_offset * size * shadow_horizontal_reach_fraction();
        floor::<u32>((peek + shadow_margin).ceil())
    }

    #[test]
    fn prepared_art_matches_the_sleeve_and_label_sides() {
        let art = synthetic_art(400);
        let canvas_side = Pixels(272);
        let prepared = prepare_art(&art, canvas_side);
        let sleeve_side = sleeve_inset_side(canvas_side);
        let label_side = label_diameter(canvas_side);
        assert_eq!(prepared.sleeve.dimensions(), (sleeve_side, sleeve_side));
        assert_eq!(prepared.label.dimensions(), (label_side, label_side));
    }

    #[test]
    fn art_of_the_wrong_size_is_resized_to_fit() {
        let art = synthetic_art(8);
        let canvas_side = Pixels(96);
        let key = VinylCacheKey {
            path: None,
            side: canvas_side,
            vinyl_style: VinylStyle::fixture(),
        };
        let mut cache = VinylCache::default();

        let image = cache.compose(&key, Some(&art));
        let peek = expected_peek(canvas_side);
        assert_eq!(image.dimensions(), (canvas_side.0 + peek, canvas_side.0));
        let center = (canvas_side.0 / 2, canvas_side.0 / 2);
        assert_ne!(
            *image.get_pixel(center.0, center.1),
            image::Rgba([0, 0, 0, 0])
        );
    }
}
