use fast_image_resize::{
    CropBox,
    PixelType,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::RgbaImage;
use tiny_skia::{FillRule, IntSize, Mask, Path, Pixmap, PixmapPaint, Transform};

use crate::pixels::{
    numeric::{dimension_f32, floor},
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
pub(crate) fn sleeve_inset_side_px(size_px: u32) -> u32 {
    let size = dimension_f32(size_px.max(1));
    let pad = VINYL_LAYOUT.sleeve_padding * size;
    floor::<u32>((size - pad * 2.0).round()).max(1)
}

fn label_diameter_px(size_px: u32) -> u32 {
    floor::<u32>((VinylGeometry::new(size_px).label_radius * 2.0).round()).max(1)
}

#[derive(Debug)]
pub(crate) struct VinylArt {
    pub(crate) sleeve: RgbaImage,
    pub(crate) label: RgbaImage,
}

#[must_use]
pub(crate) fn prepare_art(art: &RgbaImage, size_px: u32) -> VinylArt {
    let sleeve_side = sleeve_inset_side_px(size_px);
    let label_side = label_diameter_px(size_px);
    VinylArt {
        sleeve: cover_crop_resize(art, sleeve_side, sleeve_side),
        label: cover_crop_resize(art, label_side, label_side),
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
    let Some(source) = rgba_to_pixmap(&sized) else {
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

fn cover_crop_resize(
    image: &RgbaImage,
    target_width: u32,
    target_height: u32,
) -> RgbaImage {
    let (source_width, source_height) = image.dimensions();
    let (target_width, target_height) = (target_width.max(1), target_height.max(1));
    if (source_width, source_height) == (target_width, target_height) {
        return image.clone();
    }
    if source_width == 0 || source_height == 0 {
        return RgbaImage::new(target_width, target_height);
    }

    let target_aspect = dimension_f32(target_width) / dimension_f32(target_height);
    let source_aspect = dimension_f32(source_width) / dimension_f32(source_height);
    let (crop_width, crop_height) = if source_aspect > target_aspect {
        let crop_height = source_height;
        let crop_width =
            floor::<u32>((dimension_f32(source_height) * target_aspect).round())
                .clamp(1, source_width);
        (crop_width, crop_height)
    } else {
        let crop_width = source_width;
        let crop_height =
            floor::<u32>((dimension_f32(source_width) / target_aspect).round())
                .clamp(1, source_height);
        (crop_width, crop_height)
    };
    let crop = CropBox {
        left: f64::from((source_width - crop_width) / 2),
        top: f64::from((source_height - crop_height) / 2),
        width: f64::from(crop_width),
        height: f64::from(crop_height),
    };
    resample(image, crop, (target_width, target_height))
        .unwrap_or_else(|| RgbaImage::new(target_width, target_height))
}

fn resample(image: &RgbaImage, crop: CropBox, target: (u32, u32)) -> Option<RgbaImage> {
    let (source_width, source_height) = image.dimensions();
    let (target_width, target_height) = target;
    let source =
        ImageRef::new(source_width, source_height, image.as_raw(), PixelType::U8x4)
            .ok()?;
    let mut resized = Image::new(target_width, target_height, PixelType::U8x4);
    let options = ResizeOptions::new()
        .crop(crop.left, crop.top, crop.width, crop.height)
        .use_alpha(false);
    Resizer::new()
        .resize(&source, &mut resized, &options)
        .ok()?;
    RgbaImage::from_raw(target_width, target_height, resized.into_vec())
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

fn rgba_to_pixmap(image: &RgbaImage) -> Option<Pixmap> {
    let (width, height) = image.dimensions();
    let size = IntSize::from_wh(width, height)?;
    Pixmap::from_vec(image.as_raw().clone(), size)
}

#[cfg(test)]
mod tests {
    use kernel::domain::revision::Revision;

    use crate::pixels::{
        numeric::{dimension_f32, floor},
        vinyl::{
            VinylCache,
            VinylCacheKey,
            VinylStyle,
            art::{label_diameter_px, prepare_art, sleeve_inset_side_px},
            geometry::{VINYL_LAYOUT, shadow_horizontal_reach_fraction},
            test_support::synthetic_art,
        },
    };

    fn expected_peek_px(size_px: u32) -> u32 {
        let size = dimension_f32(size_px);
        let disc_diameter = VINYL_LAYOUT.disc_fraction * size;
        let peek = VINYL_LAYOUT.slide_fraction * disc_diameter;
        let shadow_margin =
            VINYL_LAYOUT.shadow_offset * size * shadow_horizontal_reach_fraction();
        floor::<u32>((peek + shadow_margin).ceil())
    }

    #[test]
    fn prepare_art_sizes_match_render_targets() {
        let art = synthetic_art(400);
        let size_px = 272;
        let prepared = prepare_art(&art, size_px);
        let sleeve_side = sleeve_inset_side_px(size_px);
        let label_side = label_diameter_px(size_px);
        assert_eq!(prepared.sleeve.dimensions(), (sleeve_side, sleeve_side));
        assert_eq!(prepared.label.dimensions(), (label_side, label_side));
    }

    #[test]
    fn art_of_the_wrong_size_is_resized_to_fit() {
        let art = synthetic_art(8);
        let size_px = 96;
        let key = VinylCacheKey {
            config_revision: Revision::default(),
            theme_revision: Revision::default(),
            path: None,
            size_px,
            colors: VinylStyle::fixture(),
        };
        let mut cache = VinylCache::default();

        let image = cache.compose(key, Some(&art));
        let peek = expected_peek_px(size_px);
        assert_eq!(image.dimensions(), (size_px + peek, size_px));
        let center = (size_px / 2, size_px / 2);
        assert_ne!(
            *image.get_pixel(center.0, center.1),
            image::Rgba([0, 0, 0, 0])
        );
    }
}
