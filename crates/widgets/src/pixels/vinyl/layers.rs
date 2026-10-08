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
    paint_label_and_ring(&mut pixmap, sleeve_input, &geometry);
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

fn paint_label_and_ring(
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
    use std::{path::PathBuf, sync::Arc};

    use kernel::domain::{appearance::Rgb, geometry::Pixels};
    use rstest::rstest;

    use crate::pixels::{
        cover::CoverImage,
        vinyl::{
            VinylCache,
            Wanted,
            art::prepare_art,
            layers::{
                SleeveInput,
                VinylFrame,
                compose_vinyl_frame,
                paint_record_layer,
                paint_sleeve_layer,
                scale_alpha,
                skia_color,
                skia_color_with_alpha,
            },
            tests::{noir_vinyl_style, synthetic_art},
        },
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
        let vinyl_style = noir_vinyl_style();
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
        let cover_image = CoverImage {
            path: PathBuf::from("/music/a.flac"),
            image: Arc::new(art),
        };
        let wanted = Wanted {
            cover_image: Some(&cover_image),
            side: canvas_side,
            vinyl_style,
        };
        let mut cache = VinylCache::default();
        assert_eq!(layered.as_ref(), Some(cache.compose(&wanted).as_raw()));
    }

    #[test]
    fn transparent_art_shows_the_paper_at_the_sleeve_centre() {
        let canvas_side = Pixels(96);
        let vinyl_style = noir_vinyl_style();
        let cover_image = CoverImage {
            path: PathBuf::from("/music/clear.flac"),
            image: Arc::new(image::RgbaImage::from_pixel(
                64,
                64,
                image::Rgba([255, 255, 255, 0]),
            )),
        };
        let wanted = Wanted {
            cover_image: Some(&cover_image),
            side: canvas_side,
            vinyl_style,
        };
        let mut cache = VinylCache::default();

        let image = cache.compose(&wanted);

        let Rgb([red, green, blue]) = vinyl_style.paper;
        assert_eq!(
            *image.get_pixel(canvas_side.0 / 2, canvas_side.0 / 2),
            image::Rgba([red, green, blue, 255])
        );
    }

    #[rstest]
    #[case::fifty_at_half(50, 0.5, 25)]
    #[case::fifty_at_three_quarters(50, 0.75, 38)]
    #[case::fifty_at_full(50, 1.0, 50)]
    #[case::two_hundred_at_a_quarter(200, 0.25, 50)]
    fn scale_alpha_scales_the_peak_by_the_fraction(
        #[case] peak: u8,
        #[case] fraction: f32,
        #[case] alpha: u8,
    ) {
        assert_eq!(scale_alpha(peak, fraction), alpha);
    }

    struct PixelRow {
        side: u32,
        x: u32,
        y: u32,
        pixel: [u8; 4],
    }

    #[rstest]
    #[case::sleeve_top_left_corner(PixelRow { side: 250, x: 0, y: 0, pixel: [111, 119, 133, 165] })]
    #[case::sleeve_top_right_corner(PixelRow { side: 250, x: 249, y: 0, pixel: [113, 120, 133, 151] })]
    #[case::sleeve_bottom_right_corner(PixelRow { side: 250, x: 249, y: 249, pixel: [89, 95, 105, 191] })]
    #[case::sleeve_bottom_left_corner(PixelRow { side: 250, x: 0, y: 249, pixel: [106, 112, 125, 175] })]
    #[case::sleeve_inside_the_rounded_corner(PixelRow { side: 250, x: 2, y: 2, pixel: [216, 221, 230, 255] })]
    #[case::sleeve_left_border(PixelRow { side: 250, x: 0, y: 125, pixel: [161, 167, 179, 255] })]
    #[case::sleeve_right_border(PixelRow { side: 250, x: 249, y: 125, pixel: [161, 167, 179, 255] })]
    #[case::art_top_left_corner(PixelRow { side: 250, x: 5, y: 5, pixel: [94, 96, 172, 255] })]
    #[case::art_inside_its_corner(PixelRow { side: 250, x: 6, y: 6, pixel: [0, 0, 128, 255] })]
    #[case::art_left_edge(PixelRow { side: 250, x: 5, y: 125, pixel: [53, 149, 153, 255] })]
    #[case::art_centre(PixelRow { side: 250, x: 125, y: 125, pixel: [126, 126, 128, 255] })]
    #[case::art_right_edge(PixelRow { side: 250, x: 244, y: 125, pixel: [243, 149, 153, 255] })]
    #[case::art_bottom_edge(PixelRow { side: 250, x: 125, y: 244, pixel: [148, 244, 153, 255] })]
    #[case::label_beside_the_sleeve(PixelRow { side: 250, x: 252, y: 125, pixel: [150, 78, 78, 255] })]
    #[case::label_border_inner(PixelRow { side: 250, x: 254, y: 125, pixel: [200, 156, 160, 255] })]
    #[case::label_border_outer(PixelRow { side: 250, x: 255, y: 125, pixel: [210, 215, 224, 255] })]
    #[case::third_groove(PixelRow { side: 250, x: 291, y: 125, pixel: [55, 57, 62, 255] })]
    #[case::second_groove(PixelRow { side: 250, x: 295, y: 125, pixel: [54, 56, 61, 255] })]
    #[case::between_grooves(PixelRow { side: 250, x: 297, y: 125, pixel: [37, 39, 44, 255] })]
    #[case::first_groove(PixelRow { side: 250, x: 298, y: 125, pixel: [55, 57, 62, 255] })]
    #[case::record_shadow_right(PixelRow { side: 250, x: 308, y: 129, pixel: [0, 0, 0, 34] })]
    #[case::record_shadow_below(PixelRow { side: 250, x: 270, y: 200, pixel: [0, 0, 0, 87] })]
    #[case::empty_top_right(PixelRow { side: 250, x: 310, y: 2, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_row_at_250(PixelRow { side: 250, x: 250, y: 20, pixel: [78, 82, 92, 177] })]
    #[case::sleeve_shadow_row_at_251(PixelRow { side: 250, x: 251, y: 20, pixel: [0, 0, 0, 99] })]
    #[case::sleeve_shadow_row_at_252(PixelRow { side: 250, x: 252, y: 20, pixel: [0, 0, 0, 99] })]
    #[case::sleeve_shadow_row_at_253(PixelRow { side: 250, x: 253, y: 20, pixel: [0, 0, 0, 79] })]
    #[case::sleeve_shadow_row_at_254(PixelRow { side: 250, x: 254, y: 20, pixel: [0, 0, 0, 34] })]
    #[case::sleeve_shadow_row_at_255(PixelRow { side: 250, x: 255, y: 20, pixel: [0, 0, 0, 7] })]
    #[case::sleeve_shadow_row_at_256(PixelRow { side: 250, x: 256, y: 20, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_row_at_258(PixelRow { side: 250, x: 258, y: 20, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_row_at_260(PixelRow { side: 250, x: 260, y: 20, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_row_at_262(PixelRow { side: 250, x: 262, y: 20, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_top_at_0(PixelRow { side: 250, x: 252, y: 0, pixel: [0, 0, 0, 0] })]
    #[case::sleeve_shadow_top_at_1(PixelRow { side: 250, x: 252, y: 1, pixel: [0, 0, 0, 10] })]
    #[case::sleeve_shadow_top_at_2(PixelRow { side: 250, x: 252, y: 2, pixel: [0, 0, 0, 30] })]
    #[case::sleeve_shadow_top_at_3(PixelRow { side: 250, x: 252, y: 3, pixel: [0, 0, 0, 60] })]
    #[case::sleeve_shadow_top_at_5(PixelRow { side: 250, x: 252, y: 5, pixel: [0, 0, 0, 99] })]
    #[case::sleeve_shadow_bottom(PixelRow { side: 250, x: 252, y: 248, pixel: [0, 0, 0, 99] })]
    #[case::sleeve_shadow_bottom_on_a_small_side(PixelRow { side: 100, x: 100, y: 80, pixel: [51, 53, 59, 255] })]
    #[case::art_corner_square_once_the_padding_passes_the_radius(PixelRow { side: 240, x: 5, y: 5, pixel: [0, 0, 128, 255] })]
    fn each_layer_paints_its_pixels_where_the_geometry_puts_them(
        #[case] row: PixelRow,
    ) {
        let PixelRow { side, x, y, pixel } = row;
        let cover_image = CoverImage {
            path: PathBuf::from("/music/a.flac"),
            image: Arc::new(synthetic_art(64)),
        };
        let wanted = Wanted {
            cover_image: Some(&cover_image),
            side: Pixels(side),
            vinyl_style: noir_vinyl_style(),
        };

        let image = VinylCache::default().compose(&wanted);

        assert_eq!(image.get_pixel(x, y).0, pixel);
    }
}
