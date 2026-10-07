use image::RgbaImage;
use kernel::domain::{appearance::Rgb, geometry::Pixels};
use tiny_skia::{Color, FillRule, Paint, Path, Pixmap, PixmapPaint, Transform};

use crate::pixels::{
    numeric::{channel_byte, dimension_f32},
    vinyl::{
        VinylStyle,
        art::{ArtClip, VinylArt, paint_art_clipped},
        geometry::{
            Disc,
            RoundedRect,
            SHADOW_BLUR_PASSES,
            SHADOW_DIRECTION,
            Stroke,
            VINYL_LAYOUT,
            VinylGeometry,
            circle_path,
            rounded_rect_path,
        },
    },
};

#[must_use]
fn skia_color(rgb: Rgb) -> Color {
    skia_color_with_alpha(rgb, 255)
}

#[must_use]
fn skia_color_with_alpha(rgb: Rgb, alpha: u8) -> Color {
    Color::from_rgba8(rgb.0[0], rgb.0[1], rgb.0[2], alpha)
}

fn scale_alpha(peak: u8, fraction: f32) -> u8 {
    channel_byte((dimension_f32(u32::from(peak)) * fraction).round())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VinylFrame {
    pub(crate) side: Pixels,
    pub(crate) style: VinylStyle,
}

#[must_use]
pub(crate) fn paint_record_layer(frame: VinylFrame) -> Option<Pixmap> {
    let geometry = VinylGeometry::new(frame.side);
    let mut pixmap = Pixmap::new(geometry.width.0, geometry.height.0)?;
    pixmap.fill(Color::TRANSPARENT);
    paint_record_and_grooves(&mut pixmap, &frame, &geometry);
    Some(pixmap)
}

pub(crate) struct SleeveInput<'a> {
    pub(crate) frame: VinylFrame,
    pub(crate) art: Option<&'a VinylArt>,
}

#[must_use]
pub(crate) fn paint_sleeve_layer(sleeve_input: &SleeveInput<'_>) -> Option<Pixmap> {
    let geometry = VinylGeometry::new(sleeve_input.frame.side);
    let mut pixmap = Pixmap::new(geometry.width.0, geometry.height.0)?;
    paint_sleeve(&mut pixmap, sleeve_input);
    Some(pixmap)
}

#[must_use]
pub(crate) fn compose_vinyl_frame(
    record: &Pixmap,
    sleeve: &Pixmap,
    sleeve_input: &SleeveInput<'_>,
) -> RgbaImage {
    let mut pixmap = record.clone();
    let geometry = VinylGeometry::new(sleeve_input.frame.side);
    paint_label_ring_spindle(&mut pixmap, sleeve_input, &geometry);
    pixmap.draw_pixmap(
        0,
        0,
        sleeve.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    let width = pixmap.width();
    let height = pixmap.height();
    let straight = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    RgbaImage::from_raw(width, height, straight)
        .unwrap_or_else(|| solid_fallback(width, height))
}

pub(crate) fn solid_fallback(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width.max(1), height.max(1), image::Rgba([0, 0, 0, 255]))
}

fn paint_record_and_grooves(
    pixmap: &mut Pixmap,
    frame: &VinylFrame,
    geometry: &VinylGeometry,
) {
    let record = geometry.record();
    paint_drop_shadow(pixmap, frame, |dx, dy, dr| {
        circle_path(Disc {
            center_x: record.center_x + dx,
            center_y: record.center_y + dy,
            radius: record.radius + dr,
        })
    });
    fill_path(pixmap, circle_path(record), skia_color(frame.style.record));
    paint_grooves(pixmap, frame, geometry);
}

fn paint_label_ring_spindle(
    pixmap: &mut Pixmap,
    sleeve_input: &SleeveInput<'_>,
    geometry: &VinylGeometry,
) {
    let label = geometry.label();
    match sleeve_input.art {
        Some(art) => {
            if let Some(clip) = ArtClip::circle(label) {
                paint_art_clipped(pixmap, &art.label, &clip);
            }
        }
        None => fill_path(
            pixmap,
            circle_path(label),
            skia_color(sleeve_input.frame.style.accent),
        ),
    }
    stroke_path(
        pixmap,
        circle_path(label),
        Stroke {
            color: skia_color(sleeve_input.frame.style.paper),
            width: VINYL_LAYOUT.label_border_width * geometry.size(),
        },
    );

    let spindle_disc = Disc {
        radius: VINYL_LAYOUT.spindle_radius_fraction * geometry.size(),
        ..label
    };
    fill_path(
        pixmap,
        circle_path(spindle_disc),
        skia_color(sleeve_input.frame.style.record),
    );
}

const GROOVE_LINE_WIDTH: f32 = 1.0;

fn paint_grooves(pixmap: &mut Pixmap, frame: &VinylFrame, geometry: &VinylGeometry) {
    let record = geometry.record();
    let gap = VINYL_LAYOUT.groove_spacing * geometry.size();
    for i in 0..VINYL_LAYOUT.groove_count {
        let radius = record.radius - gap * dimension_f32(i + 1);
        if radius <= geometry.label_radius {
            break;
        }
        let alpha = if i % 2 == 0 {
            VINYL_LAYOUT.groove_alpha
        } else {
            VINYL_LAYOUT.groove_alpha / 2
        };
        stroke_path(
            pixmap,
            circle_path(Disc { radius, ..record }),
            Stroke {
                color: skia_color_with_alpha(frame.style.groove, alpha),
                width: GROOVE_LINE_WIDTH,
            },
        );
    }
}

fn paint_sleeve(pixmap: &mut Pixmap, sleeve_input: &SleeveInput<'_>) {
    let size = dimension_f32(sleeve_input.frame.side.0.max(1));
    let layout = VINYL_LAYOUT;
    let rect = RoundedRect {
        x: 0.0,
        y: 0.0,
        width: size,
        height: size,
        radius: layout.corner_radius * size,
    };

    paint_drop_shadow(pixmap, &sleeve_input.frame, |dx, dy, dr| {
        rounded_rect_path(RoundedRect {
            x: rect.x + dx - dr / 2.0,
            y: rect.y + dy - dr / 2.0,
            width: rect.width + dr,
            height: rect.height + dr,
            radius: rect.radius,
        })
    });

    fill_path(
        pixmap,
        rounded_rect_path(rect),
        skia_color(sleeve_input.frame.style.paper),
    );

    let pad = layout.sleeve_padding * size;
    let inset_rect = RoundedRect {
        x: rect.x + pad,
        y: rect.y + pad,
        width: rect.width - pad * 2.0,
        height: rect.height - pad * 2.0,
        radius: (rect.radius - pad).max(0.0),
    };
    if let (Some(art), Some(clip)) =
        (sleeve_input.art, ArtClip::rounded_rect(inset_rect))
    {
        paint_art_clipped(pixmap, &art.sleeve, &clip);
    }
    stroke_path(
        pixmap,
        rounded_rect_path(rect),
        Stroke {
            color: skia_color(sleeve_input.frame.style.border),
            width: layout.border_width * size,
        },
    );
}

fn paint_drop_shadow(
    pixmap: &mut Pixmap,
    frame: &VinylFrame,
    shape: impl Fn(f32, f32, f32) -> Option<Path>,
) {
    let offset = VINYL_LAYOUT.shadow_offset * dimension_f32(frame.side.0.max(1));
    let (dx, dy) = (offset * SHADOW_DIRECTION.0, offset * SHADOW_DIRECTION.1);
    for (spread_fraction, alpha_fraction) in SHADOW_BLUR_PASSES {
        let alpha = scale_alpha(VINYL_LAYOUT.shadow_alpha, alpha_fraction);
        fill_path(
            pixmap,
            shape(dx, dy, offset * spread_fraction),
            skia_color_with_alpha(frame.style.shadow, alpha),
        );
    }
}

fn fill_path(pixmap: &mut Pixmap, path: Option<Path>, fill: Color) {
    let Some(path) = path else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(fill);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn stroke_path(pixmap: &mut Pixmap, path: Option<Path>, stroke: Stroke) {
    let Some(path) = path else {
        return;
    };
    if stroke.width <= 0.0 {
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(stroke.color);
    paint.anti_alias = true;
    let skia_stroke = tiny_skia::Stroke {
        width: stroke.width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &skia_stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use kernel::domain::{appearance::Rgb, geometry::Pixels};
    use rstest::rstest;

    use crate::pixels::vinyl::{
        VinylCache,
        VinylCacheKey,
        VinylStyle,
        art::prepare_art,
        layers::{
            SleeveInput,
            VinylFrame,
            compose_vinyl_frame,
            paint_record_layer,
            paint_sleeve_layer,
            skia_color,
            skia_color_with_alpha,
        },
        test_support::synthetic_art,
    };

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

    #[test]
    fn composing_the_layers_matches_the_cache() {
        let art = synthetic_art(64);
        let canvas_side = Pixels(96);
        let vinyl_style = VinylStyle::fixture();
        let prepared = prepare_art(&art, canvas_side);
        let frame = VinylFrame {
            side: canvas_side,
            style: vinyl_style,
        };
        let sleeve_input = SleeveInput {
            frame,
            art: prepared.as_ref(),
        };
        let layered = paint_record_layer(frame)
            .zip(paint_sleeve_layer(&sleeve_input))
            .map(|(record, sleeve)| {
                compose_vinyl_frame(&record, &sleeve, &sleeve_input).into_raw()
            });
        let key = VinylCacheKey {
            path: None,
            side: canvas_side,
            vinyl_style,
        };
        let mut cache = VinylCache::default();
        assert_eq!(
            layered.as_ref(),
            Some(cache.compose(&key, Some(&art)).as_raw())
        );
    }

    #[test]
    fn transparent_art_shows_the_paper_at_the_sleeve_centre() {
        let canvas_side = Pixels(96);
        let vinyl_style = VinylStyle::fixture();
        let art = image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 0]));
        let key = VinylCacheKey {
            path: Some(std::path::PathBuf::from("/music/clear.flac")),
            side: canvas_side,
            vinyl_style,
        };
        let mut cache = VinylCache::default();

        let image = cache.compose(&key, Some(&art));

        let Rgb([red, green, blue]) = vinyl_style.paper;
        assert_eq!(
            *image.get_pixel(canvas_side.0 / 2, canvas_side.0 / 2),
            image::Rgba([red, green, blue, 255])
        );
    }
}
