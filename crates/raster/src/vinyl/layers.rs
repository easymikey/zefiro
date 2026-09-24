use image::RgbaImage;
use tiny_skia::{
    Color,
    FillRule,
    Paint,
    PathBuilder,
    Pixmap,
    PixmapPaint,
    Stroke,
    Transform,
};

use crate::{
    numeric::dimension_f32,
    paint::{skia_color, skia_color_with_alpha},
    vinyl::{
        SleeveFace,
        VinylArt,
        VinylColors,
        VinylFrame,
        VinylLayout,
        art::{paint_art_in_circle, paint_art_in_rounded_rect},
        colors::scale_alpha,
        geometry::{
            Disc,
            LabelArt,
            RecordGeometry,
            RoundedRect,
            SHADOW_BLUR_PASSES,
            SHADOW_DIRECTION,
            ShadowStyle,
            SleevePlacement,
            StrokeStyle,
            VinylGeometry,
            canvas_dims,
            record_disc,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VinylFrameStyle {
    pub size_px: u32,
    pub colors: VinylColors,
    pub layout: VinylLayout,
}

#[derive(Debug)]
pub(crate) struct VinylFrameBase {
    pixmap: Pixmap,
}

#[must_use]
pub(crate) fn prepare_frame_base(style: VinylFrameStyle) -> Option<VinylFrameBase> {
    let dims = canvas_dims(&style);
    let mut pixmap = Pixmap::new(dims.width, dims.height)?;
    pixmap.fill(Color::TRANSPARENT);
    let geometry = VinylGeometry::new(dims.height, &style.layout);
    paint_record_and_grooves(&mut pixmap, &style, &geometry);
    Some(VinylFrameBase { pixmap })
}

pub(crate) struct VinylSleeve<'a> {
    pub style: VinylFrameStyle,
    pub face: SleeveFace,
    pub art: Option<&'a VinylArt>,
}

#[derive(Debug)]
pub(crate) struct VinylSleeveOverlay {
    pixmap: Pixmap,
}

#[must_use]
pub(crate) fn prepare_sleeve_overlay(
    input: &VinylSleeve<'_>,
) -> Option<VinylSleeveOverlay> {
    let dims = canvas_dims(&input.style);
    let mut pixmap = Pixmap::new(dims.width, dims.height)?;
    let geometry = VinylGeometry::new(dims.height, &input.style.layout);
    paint_sleeve(
        &mut pixmap,
        input,
        SleevePlacement {
            x: 0.0,
            y: 0.0,
            side: geometry.size,
        },
    );
    Some(VinylSleeveOverlay { pixmap })
}

#[must_use]
pub(crate) fn compose_vinyl_frame(
    base: &VinylFrameBase,
    overlay: &VinylSleeveOverlay,
    input: &VinylFrame<'_>,
) -> RgbaImage {
    let mut pixmap = base.pixmap.clone();
    let geometry = VinylGeometry::new(input.size_px.max(1), &input.layout);
    paint_label_ring_spindle(&mut pixmap, input, &geometry);
    pixmap.draw_pixmap(
        0,
        0,
        overlay.pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    let width = pixmap.width();
    let height = pixmap.height();
    pixmap_to_rgba(pixmap, width, height)
}

pub(crate) fn solid_fallback(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width.max(1), height.max(1), image::Rgba([0, 0, 0, 255]))
}

fn pixmap_to_rgba(pixmap: Pixmap, width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_raw(width, height, pixmap.take())
        .unwrap_or_else(|| solid_fallback(width, height))
}

fn sleeve_art<'a>(input: &VinylFrame<'a>) -> Option<&'a VinylArt> {
    match input.face {
        SleeveFace::Art => input.art,
        SleeveFace::Blank => None,
    }
}

fn paint_record_and_grooves(
    pixmap: &mut Pixmap,
    style: &VinylFrameStyle,
    geometry: &VinylGeometry,
) {
    let record = record_disc(geometry);
    let shadow = ShadowStyle {
        color: style.colors.shadow,
        offset: style.layout.shadow_offset * geometry.size,
        peak_alpha: style.layout.shadow_alpha,
    };
    paint_drop_shadow_disc(pixmap, record, shadow);
    fill_disc(pixmap, record, skia_color(style.colors.record));

    let label_r = record.r * style.layout.label_radius_fraction;
    paint_grooves(pixmap, style, RecordGeometry { record, label_r });
}

fn paint_label_ring_spindle(
    pixmap: &mut Pixmap,
    input: &VinylFrame<'_>,
    geometry: &VinylGeometry,
) {
    let record = record_disc(geometry);
    let layout = &input.layout;
    let label_r = record.r * layout.label_radius_fraction;
    let label = Disc {
        cx: record.cx,
        cy: record.cy,
        r: label_r,
    };
    match sleeve_art(input) {
        Some(art) => paint_art_in_circle(
            pixmap,
            &LabelArt {
                art: &art.label,
                disc: label,
            },
        ),
        None => fill_disc(pixmap, label, skia_color(input.colors.accent)),
    }
    stroke_ring(
        pixmap,
        label,
        StrokeStyle {
            color: skia_color(input.colors.paper),
            width: layout.label_border_width * geometry.size,
        },
    );

    let spindle = Disc {
        cx: record.cx,
        cy: record.cy,
        r: layout.spindle_radius_fraction * geometry.size,
    };
    fill_disc(pixmap, spindle, skia_color(input.colors.record));
}

const GROOVE_LINE_WIDTH: f32 = 1.0;

fn paint_grooves(
    pixmap: &mut Pixmap,
    style: &VinylFrameStyle,
    geometry: RecordGeometry,
) {
    let size = dimension_f32(style.size_px.max(1));
    let gap = style.layout.groove_spacing * size;
    for i in 0..style.layout.groove_count {
        let r = geometry.record.r - gap * dimension_f32(i + 1);
        if r <= geometry.label_r {
            break;
        }
        let alpha = if i % 2 == 0 {
            style.layout.groove_alpha
        } else {
            style.layout.groove_alpha / 2
        };
        stroke_ring(
            pixmap,
            Disc {
                r,
                ..geometry.record
            },
            StrokeStyle {
                color: skia_color_with_alpha(style.colors.groove, alpha),
                width: GROOVE_LINE_WIDTH,
            },
        );
    }
}

fn paint_sleeve(
    pixmap: &mut Pixmap,
    input: &VinylSleeve<'_>,
    placement: SleevePlacement,
) {
    let size = dimension_f32(input.style.size_px.max(1));
    let layout = &input.style.layout;
    let corner_radius = layout.corner_radius * size;
    let border_width = layout.border_width * size;
    let rect = RoundedRect {
        x: placement.x,
        y: placement.y,
        width: placement.side,
        height: placement.side,
        radius: corner_radius,
    };

    let shadow = ShadowStyle {
        color: input.style.colors.shadow,
        offset: layout.shadow_offset * size,
        peak_alpha: layout.shadow_alpha,
    };
    paint_drop_shadow_rounded_rect(pixmap, rect, shadow);

    fill_rounded_rect(pixmap, rect, skia_color(input.style.colors.paper));

    let pad = layout.sleeve_padding * size;
    let inset = RoundedRect {
        x: rect.x + pad,
        y: rect.y + pad,
        width: rect.width - pad * 2.0,
        height: rect.height - pad * 2.0,
        radius: (rect.radius - pad).max(0.0),
    };
    match input.face {
        SleeveFace::Art => {
            if let Some(art) = input.art {
                paint_art_in_rounded_rect(pixmap, &art.sleeve, inset);
            }
        }
        SleeveFace::Blank => {
            fill_rounded_rect(
                pixmap,
                inset,
                skia_color(input.style.colors.blank_paper),
            );
        }
    }
    stroke_rounded_rect(
        pixmap,
        rect,
        StrokeStyle {
            color: skia_color(input.style.colors.border),
            width: border_width,
        },
    );
}

fn paint_drop_shadow_disc(pixmap: &mut Pixmap, disc: Disc, shadow: ShadowStyle) {
    let (dx, dy) = (
        shadow.offset * SHADOW_DIRECTION.0,
        shadow.offset * SHADOW_DIRECTION.1,
    );
    for (spread_fraction, alpha_fraction) in SHADOW_BLUR_PASSES {
        fill_disc(
            pixmap,
            Disc {
                cx: disc.cx + dx,
                cy: disc.cy + dy,
                r: disc.r + shadow.offset * spread_fraction,
            },
            skia_color_with_alpha(
                shadow.color,
                scale_alpha(shadow.peak_alpha, alpha_fraction),
            ),
        );
    }
}

fn paint_drop_shadow_rounded_rect(
    pixmap: &mut Pixmap,
    rect: RoundedRect,
    shadow: ShadowStyle,
) {
    let (dx, dy) = (
        shadow.offset * SHADOW_DIRECTION.0,
        shadow.offset * SHADOW_DIRECTION.1,
    );
    for (spread_fraction, alpha_fraction) in SHADOW_BLUR_PASSES {
        let dr = shadow.offset * spread_fraction;
        fill_rounded_rect(
            pixmap,
            RoundedRect {
                x: rect.x + dx - dr / 2.0,
                y: rect.y + dy - dr / 2.0,
                width: rect.width + dr,
                height: rect.height + dr,
                radius: rect.radius,
            },
            skia_color_with_alpha(
                shadow.color,
                scale_alpha(shadow.peak_alpha, alpha_fraction),
            ),
        );
    }
}

pub(crate) fn rounded_rect_path(rect: RoundedRect) -> Option<tiny_skia::Path> {
    let RoundedRect {
        x,
        y,
        width,
        height,
        radius,
    } = rect;
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

fn fill_rounded_rect(pixmap: &mut Pixmap, rect: RoundedRect, fill: Color) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(path) = rounded_rect_path(rect) else {
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

fn stroke_rounded_rect(pixmap: &mut Pixmap, rect: RoundedRect, style: StrokeStyle) {
    if rect.width <= 0.0 || rect.height <= 0.0 || style.width <= 0.0 {
        return;
    }
    let Some(path) = rounded_rect_path(rect) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(style.color);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: style.width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn fill_disc(pixmap: &mut Pixmap, disc: Disc, fill: Color) {
    if disc.r <= 0.0 {
        return;
    }
    let mut path_builder = PathBuilder::new();
    path_builder.push_circle(disc.cx, disc.cy, disc.r);
    let Some(path) = path_builder.finish() else {
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

fn stroke_ring(pixmap: &mut Pixmap, disc: Disc, style: StrokeStyle) {
    if disc.r <= 0.0 || style.width <= 0.0 {
        return;
    }
    let mut path_builder = PathBuilder::new();
    path_builder.push_circle(disc.cx, disc.cy, disc.r);
    let Some(path) = path_builder.finish() else {
        return;
    };
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
    use crate::vinyl::{
        SleeveFace,
        VinylColors,
        VinylFrame,
        VinylLayout,
        fixtures::synthetic_art,
        layers::{
            VinylFrameStyle,
            VinylSleeve,
            compose_vinyl_frame,
            prepare_frame_base,
            prepare_sleeve_overlay,
        },
        prepare_art,
        vinyl_image,
    };

    #[test]
    fn cached_layer_path_matches_from_scratch_vinyl_image() {
        let art = synthetic_art(64);
        let size_px = 96;
        let layout = VinylLayout::default();
        let colors = VinylColors::default();
        let prepared = prepare_art(&art, size_px, &layout);
        let style = VinylFrameStyle {
            size_px,
            colors,
            layout,
        };

        for face in [SleeveFace::Art, SleeveFace::Blank] {
            let art_for_face = match face {
                SleeveFace::Art => Some(&prepared),
                SleeveFace::Blank => None,
            };
            let sleeve_input = VinylSleeve {
                style,
                face,
                art: art_for_face,
            };
            let input = VinylFrame {
                art: art_for_face,
                size_px,
                face,
                colors,
                layout,
            };
            let cached = prepare_frame_base(style)
                .zip(prepare_sleeve_overlay(&sleeve_input))
                .map(|(base, overlay)| {
                    compose_vinyl_frame(&base, &overlay, &input).into_raw()
                });
            assert_eq!(
                Some(vinyl_image(&input).into_raw()),
                cached,
                "cached path diverged from vinyl_image at face={face:?}"
            );
        }
    }
}
