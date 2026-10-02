use image::RgbaImage;
use kernel::domain::appearance::Rgb;
use tiny_skia::{
    Color,
    FillRule,
    Paint,
    Path,
    PathBuilder,
    Pixmap,
    PixmapPaint,
    Stroke,
    Transform,
};

use crate::pixels::{
    numeric::{channel_byte, dimension_f32},
    vinyl::{
        VinylArt,
        VinylStyle,
        art::{ArtClip, paint_art_clipped},
        geometry::{
            Disc,
            RoundedRect,
            SHADOW_BLUR_PASSES,
            SHADOW_DIRECTION,
            StrokeStyle,
            VINYL_LAYOUT,
            VinylGeometry,
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
pub(crate) struct VinylFrameStyle {
    pub size_px: u32,
    pub colors: VinylStyle,
}

#[must_use]
pub(crate) fn paint_record_layer(style: VinylFrameStyle) -> Option<Pixmap> {
    let geometry = VinylGeometry::new(style.size_px);
    let mut pixmap = Pixmap::new(geometry.width_px, geometry.height_px)?;
    pixmap.fill(Color::TRANSPARENT);
    paint_record_and_grooves(&mut pixmap, &style, &geometry);
    Some(pixmap)
}

pub(crate) struct VinylParts<'a> {
    pub style: VinylFrameStyle,
    pub art: Option<&'a VinylArt>,
}

#[must_use]
pub(crate) fn paint_sleeve_layer(parts: &VinylParts<'_>) -> Option<Pixmap> {
    let geometry = VinylGeometry::new(parts.style.size_px);
    let mut pixmap = Pixmap::new(geometry.width_px, geometry.height_px)?;
    paint_sleeve(&mut pixmap, parts);
    Some(pixmap)
}

#[must_use]
pub(crate) fn compose_vinyl_frame(
    record: &Pixmap,
    sleeve: &Pixmap,
    parts: &VinylParts<'_>,
) -> RgbaImage {
    let mut pixmap = record.clone();
    let geometry = VinylGeometry::new(parts.style.size_px);
    paint_label_ring_spindle(&mut pixmap, parts, &geometry);
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
    RgbaImage::from_raw(width, height, pixmap.take())
        .unwrap_or_else(|| solid_fallback(width, height))
}

pub(crate) fn solid_fallback(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width.max(1), height.max(1), image::Rgba([0, 0, 0, 255]))
}

fn paint_record_and_grooves(
    pixmap: &mut Pixmap,
    style: &VinylFrameStyle,
    geometry: &VinylGeometry,
) {
    let record = geometry.record();
    paint_drop_shadow(pixmap, style, |dx, dy, dr| {
        circle_path(Disc {
            center_x: record.center_x + dx,
            center_y: record.center_y + dy,
            radius: record.radius + dr,
        })
    });
    fill_path(pixmap, circle_path(record), skia_color(style.colors.record));
    paint_grooves(pixmap, style, geometry);
}

fn paint_label_ring_spindle(
    pixmap: &mut Pixmap,
    parts: &VinylParts<'_>,
    geometry: &VinylGeometry,
) {
    let label = geometry.label();
    match parts.art {
        Some(art) => {
            if let Some(clip) = ArtClip::circle(label) {
                paint_art_clipped(pixmap, &art.label, &clip);
            }
        }
        None => fill_path(
            pixmap,
            circle_path(label),
            skia_color(parts.style.colors.accent),
        ),
    }
    stroke_path(
        pixmap,
        circle_path(label),
        StrokeStyle {
            color: skia_color(parts.style.colors.paper),
            width: VINYL_LAYOUT.label_border_width * geometry.size(),
        },
    );

    let spindle = Disc {
        radius: VINYL_LAYOUT.spindle_radius_fraction * geometry.size(),
        ..label
    };
    fill_path(
        pixmap,
        circle_path(spindle),
        skia_color(parts.style.colors.record),
    );
}

const GROOVE_LINE_WIDTH: f32 = 1.0;

fn paint_grooves(
    pixmap: &mut Pixmap,
    style: &VinylFrameStyle,
    geometry: &VinylGeometry,
) {
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
            StrokeStyle {
                color: skia_color_with_alpha(style.colors.groove, alpha),
                width: GROOVE_LINE_WIDTH,
            },
        );
    }
}

fn paint_sleeve(pixmap: &mut Pixmap, parts: &VinylParts<'_>) {
    let size = dimension_f32(parts.style.size_px.max(1));
    let layout = VINYL_LAYOUT;
    let rect = RoundedRect {
        x: 0.0,
        y: 0.0,
        width: size,
        height: size,
        radius: layout.corner_radius * size,
    };

    paint_drop_shadow(pixmap, &parts.style, |dx, dy, dr| {
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
        skia_color(parts.style.colors.paper),
    );

    let pad = layout.sleeve_padding * size;
    let inset = RoundedRect {
        x: rect.x + pad,
        y: rect.y + pad,
        width: rect.width - pad * 2.0,
        height: rect.height - pad * 2.0,
        radius: (rect.radius - pad).max(0.0),
    };
    if let (Some(art), Some(clip)) = (parts.art, ArtClip::rounded_rect(inset)) {
        paint_art_clipped(pixmap, &art.sleeve, &clip);
    }
    stroke_path(
        pixmap,
        rounded_rect_path(rect),
        StrokeStyle {
            color: skia_color(parts.style.colors.border),
            width: layout.border_width * size,
        },
    );
}

fn paint_drop_shadow(
    pixmap: &mut Pixmap,
    style: &VinylFrameStyle,
    shape: impl Fn(f32, f32, f32) -> Option<Path>,
) {
    let offset = VINYL_LAYOUT.shadow_offset * dimension_f32(style.size_px.max(1));
    let (dx, dy) = (offset * SHADOW_DIRECTION.0, offset * SHADOW_DIRECTION.1);
    for (spread_fraction, alpha_fraction) in SHADOW_BLUR_PASSES {
        let alpha = scale_alpha(VINYL_LAYOUT.shadow_alpha, alpha_fraction);
        fill_path(
            pixmap,
            shape(dx, dy, offset * spread_fraction),
            skia_color_with_alpha(style.colors.shadow, alpha),
        );
    }
}

pub(crate) fn rounded_rect_path(rect: RoundedRect) -> Option<Path> {
    let RoundedRect {
        x,
        y,
        width,
        height,
        radius,
    } = rect;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let r = radius.max(0.0).min(width / 2.0).min(height / 2.0);
    let mut path_builder = PathBuilder::new();
    path_builder.move_to(x + r, y);
    path_builder.line_to(x + width - r, y);
    path_builder.quad_to(x + width, y, x + width, y + r);
    path_builder.line_to(x + width, y + height - r);
    path_builder.quad_to(x + width, y + height, x + width - r, y + height);
    path_builder.line_to(x + r, y + height);
    path_builder.quad_to(x, y + height, x, y + height - r);
    path_builder.line_to(x, y + r);
    path_builder.quad_to(x, y, x + r, y);
    path_builder.close();
    path_builder.finish()
}

pub(crate) fn circle_path(disc: Disc) -> Option<Path> {
    if disc.radius <= 0.0 {
        return None;
    }
    let mut path_builder = PathBuilder::new();
    path_builder.push_circle(disc.center_x, disc.center_y, disc.radius);
    path_builder.finish()
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

fn stroke_path(pixmap: &mut Pixmap, path: Option<Path>, style: StrokeStyle) {
    let Some(path) = path else {
        return;
    };
    if style.width <= 0.0 {
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(style.color);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: style.width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;
    use rstest::rstest;

    use crate::pixels::vinyl::{
        VinylStyle,
        compose_uncached,
        layers::{
            VinylFrameStyle,
            VinylParts,
            compose_vinyl_frame,
            paint_record_layer,
            paint_sleeve_layer,
            skia_color,
            skia_color_with_alpha,
        },
        prepare_art,
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
    fn composing_the_layers_matches_composing_uncached() {
        let art = synthetic_art(64);
        let size_px = 96;
        let colors = VinylStyle::fixture();
        let prepared = prepare_art(&art, size_px);
        let style = VinylFrameStyle { size_px, colors };
        let parts = VinylParts {
            style,
            art: Some(&prepared),
        };
        let cached = paint_record_layer(style)
            .zip(paint_sleeve_layer(&parts))
            .map(|(record, sleeve)| {
                compose_vinyl_frame(&record, &sleeve, &parts).into_raw()
            });
        assert_eq!(Some(compose_uncached(&parts).into_raw()), cached);
    }
}
