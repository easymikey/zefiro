use fast_image_resize::{
    CropBox,
    PixelType,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::RgbaImage;
use tiny_skia::{FillRule, IntSize, Mask, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::{
    numeric::{dimension_f32, dimension_u32},
    vinyl::{
        VinylLayout,
        geometry::{LabelArt, RoundedRect},
        layers::rounded_rect_path,
    },
};

#[must_use]
pub(crate) fn sleeve_inset_side_px(size_px: u32, layout: &VinylLayout) -> u32 {
    let size = dimension_f32(size_px.max(1));
    let pad = layout.sleeve_padding * size;
    dimension_u32((size - pad * 2.0).round()).max(1)
}

fn label_diameter_px(size_px: u32, layout: &VinylLayout) -> u32 {
    let size = dimension_f32(size_px.max(1));
    let record_r = layout.disc_fraction * size / 2.0;
    let label_r = record_r * layout.label_radius_fraction;
    dimension_u32((label_r * 2.0).round()).max(1)
}

#[derive(Debug)]
pub(crate) struct VinylArt {
    pub(crate) sleeve: RgbaImage,
    pub(crate) label: RgbaImage,
}

#[must_use]
pub(crate) fn prepare_art(
    art: &RgbaImage,
    size_px: u32,
    layout: &VinylLayout,
) -> VinylArt {
    let sleeve_side = sleeve_inset_side_px(size_px, layout);
    let label_side = label_diameter_px(size_px, layout);
    VinylArt {
        sleeve: cover_crop_resize(art, sleeve_side, sleeve_side),
        label: cover_crop_resize(art, label_side, label_side),
    }
}

pub(crate) fn paint_art_in_rounded_rect(
    pixmap: &mut Pixmap,
    art: &RgbaImage,
    rect: RoundedRect,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let target_width = dimension_u32(rect.width.round()).max(1);
    let target_height = dimension_u32(rect.height.round()).max(1);
    let sized = sized_or_resized(art, target_width, target_height);
    let Some(source) = rgba_to_pixmap(&sized) else {
        return;
    };
    let Some(clip_path) = rounded_rect_path(rect) else {
        return;
    };
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    mask.fill_path(&clip_path, FillRule::Winding, true, Transform::identity());
    pixmap.draw_pixmap(
        round_to_i32(rect.x),
        round_to_i32(rect.y),
        source.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        Some(&mask),
    );
}

pub(crate) fn paint_art_in_circle(pixmap: &mut Pixmap, placement: &LabelArt<'_>) {
    let disc = placement.disc;
    if disc.r <= 0.0 {
        return;
    }
    let diameter = dimension_u32((disc.r * 2.0).round()).max(1);
    let sized = sized_or_resized(placement.art, diameter, diameter);
    let Some(source) = rgba_to_pixmap(&sized) else {
        return;
    };
    let mut path_builder = PathBuilder::new();
    path_builder.push_circle(disc.cx, disc.cy, disc.r);
    let Some(clip_path) = path_builder.finish() else {
        return;
    };
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    mask.fill_path(&clip_path, FillRule::Winding, true, Transform::identity());

    let half = dimension_f32(diameter) / 2.0;
    let transform = Transform::from_translate(disc.cx - half, disc.cy - half);
    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint::default(),
        transform,
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
            dimension_u32((dimension_f32(source_height) * target_aspect).round())
                .clamp(1, source_width);
        (crop_width, crop_height)
    } else {
        let crop_width = source_width;
        let crop_height =
            dimension_u32((dimension_f32(source_width) / target_aspect).round())
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

fn round_to_i32(v: f32) -> i32 {
    crate::numeric::round_i32(v)
}

#[cfg(test)]
mod tests {
    use crate::{
        numeric::{dimension_f32, floor_u32},
        vinyl::{
            SleeveFace,
            VinylColors,
            VinylFrame,
            VinylLayout,
            art::{
                VinylArt,
                cover_crop_resize,
                label_diameter_px,
                prepare_art,
                sleeve_inset_side_px,
            },
            fixtures::synthetic_art,
            geometry::shadow_horizontal_reach_fraction,
            vinyl_image,
        },
    };

    fn expected_peek_px(size_px: u32, layout: &VinylLayout) -> u32 {
        let size = dimension_f32(size_px);
        let disc_diameter = layout.disc_fraction * size;
        let peek = layout.slide_fraction * disc_diameter;
        let shadow_margin =
            layout.shadow_offset * size * shadow_horizontal_reach_fraction();
        floor_u32((peek + shadow_margin).ceil())
    }

    #[test]
    fn prepare_art_sizes_match_render_targets() {
        let art = synthetic_art(400);
        let layout = VinylLayout::default();
        let size_px = 272;
        let prepared = prepare_art(&art, size_px, &layout);
        let sleeve_side = sleeve_inset_side_px(size_px, &layout);
        let label_side = label_diameter_px(size_px, &layout);
        assert_eq!(prepared.sleeve.dimensions(), (sleeve_side, sleeve_side));
        assert_eq!(prepared.label.dimensions(), (label_side, label_side));
    }

    #[test]
    fn mismatched_prepared_art_still_renders_via_fallback() {
        let art = synthetic_art(400);
        let size_px = 96;
        let layout = VinylLayout::default();
        let mismatched = VinylArt {
            sleeve: cover_crop_resize(&art, 8, 8),
            label: cover_crop_resize(&art, 8, 8),
        };
        let input = VinylFrame {
            art: Some(&mismatched),
            size_px,
            face: SleeveFace::Art,
            colors: VinylColors::default(),
            layout,
        };

        let image = vinyl_image(&input);
        let peek = expected_peek_px(size_px, &layout);
        assert_eq!(image.dimensions(), (size_px + peek, size_px));
        let center = (size_px / 2, size_px / 2);
        assert_ne!(
            *image.get_pixel(center.0, center.1),
            image::Rgba([0, 0, 0, 0])
        );
    }
}
