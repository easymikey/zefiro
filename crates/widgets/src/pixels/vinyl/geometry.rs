use kernel::domain::geometry::Pixels;
use tiny_skia::{Path, PathBuilder};

use crate::pixels::numeric::{dimension_f32, floor};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VinylLayout {
    pub(crate) disc_fraction: f32,
    pub(crate) slide_fraction: f32,
    pub(crate) groove_count: u32,
    pub(crate) groove_spacing: f32,
    pub(crate) label_radius_fraction: f32,
    pub(crate) spindle_radius_fraction: f32,
    pub(crate) border_width: f32,
    pub(crate) label_border_width: f32,
    pub(crate) sleeve_padding: f32,
    pub(crate) corner_radius: f32,
    pub(crate) shadow_offset: f32,
    pub(crate) groove_alpha: u8,
    pub(crate) shadow_alpha: u8,
}

pub(crate) const VINYL_LAYOUT: VinylLayout = VinylLayout {
    disc_fraction: 0.70,
    slide_fraction: 0.30,
    groove_count: 8,
    groove_spacing: 0.0140,
    label_radius_fraction: 0.46,
    spindle_radius_fraction: 0.0083,
    border_width: 0.0042,
    label_border_width: 0.0063,
    sleeve_padding: 0.0208,
    corner_radius: 0.0125,
    shadow_offset: 0.0208,
    groove_alpha: 40,
    shadow_alpha: 50,
};

#[must_use]
pub(crate) fn canvas_aspect_ratio() -> f32 {
    1.0 + VINYL_LAYOUT.slide_fraction * VINYL_LAYOUT.disc_fraction
        + VINYL_LAYOUT.shadow_offset * shadow_horizontal_reach_fraction()
}

pub(crate) const SHADOW_BLUR_PASSES: [(f32, f32); 3] =
    [(1.0, 0.5), (0.6, 0.75), (0.3, 1.0)];

pub(crate) const SHADOW_DIRECTION: (f32, f32) = (0.5, 0.8);

pub(crate) fn shadow_horizontal_reach_fraction() -> f32 {
    let widest_spread_fraction = SHADOW_BLUR_PASSES
        .iter()
        .map(|(spread_fraction, _)| *spread_fraction)
        .fold(0.0_f32, f32::max);
    SHADOW_DIRECTION.0 + widest_spread_fraction
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Disc {
    pub(crate) center_x: f32,
    pub(crate) center_y: f32,
    pub(crate) radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RoundedRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stroke {
    pub(crate) color: tiny_skia::Color,
    pub(crate) width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VinylGeometry {
    pub(crate) width: Pixels,
    pub(crate) height: Pixels,
    pub(crate) record_radius: f32,
    pub(crate) label_radius: f32,
}

impl VinylGeometry {
    pub(crate) fn new(canvas_side: Pixels) -> Self {
        let height = canvas_side.0.max(1);
        let size = dimension_f32(height);
        let record_radius = VINYL_LAYOUT.disc_fraction * size / 2.0;
        let peek = VINYL_LAYOUT.slide_fraction * (record_radius * 2.0);
        let shadow_margin =
            VINYL_LAYOUT.shadow_offset * size * shadow_horizontal_reach_fraction();
        Self {
            width: Pixels(
                height.saturating_add(floor::<u32>((peek + shadow_margin).ceil())),
            ),
            height: Pixels(height),
            record_radius,
            label_radius: record_radius * VINYL_LAYOUT.label_radius_fraction,
        }
    }

    pub(crate) fn size(&self) -> f32 {
        dimension_f32(self.height.0)
    }

    pub(crate) fn record(&self) -> Disc {
        let peek = VINYL_LAYOUT.slide_fraction * (self.record_radius * 2.0);
        Disc {
            center_x: self.size() + peek - self.record_radius,
            center_y: self.size() / 2.0,
            radius: self.record_radius,
        }
    }

    pub(crate) fn label(&self) -> Disc {
        Disc {
            radius: self.label_radius,
            ..self.record()
        }
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
    let corner_radius = radius.max(0.0).min(width / 2.0).min(height / 2.0);
    let mut path_builder = PathBuilder::new();
    path_builder.move_to(x + corner_radius, y);
    path_builder.line_to(x + width - corner_radius, y);
    path_builder.quad_to(x + width, y, x + width, y + corner_radius);
    path_builder.line_to(x + width, y + height - corner_radius);
    path_builder.quad_to(x + width, y + height, x + width - corner_radius, y + height);
    path_builder.line_to(x + corner_radius, y + height);
    path_builder.quad_to(x, y + height, x, y + height - corner_radius);
    path_builder.line_to(x, y + corner_radius);
    path_builder.quad_to(x, y, x + corner_radius, y);
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

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Pixels;

    use crate::pixels::{
        numeric::dimension_f32,
        vinyl::geometry::{
            VINYL_LAYOUT,
            VinylGeometry,
            shadow_horizontal_reach_fraction,
        },
    };

    #[test]
    fn disc_and_shadow_fit_inside_the_canvas() {
        for canvas_side in [Pixels(96), Pixels(160)] {
            let geometry = VinylGeometry::new(canvas_side);
            let disc = geometry.record();
            let shadow_offset = VINYL_LAYOUT.shadow_offset * geometry.size();
            let shadow_right_edge = disc.center_x
                + disc.radius
                + shadow_offset * shadow_horizontal_reach_fraction();
            let canvas_width = dimension_f32(geometry.width.0);
            assert!(
                shadow_right_edge <= canvas_width,
                "disc+shadow right edge {shadow_right_edge} exceeds canvas width \
                 {canvas_width} at canvas_side={}",
                canvas_side.0
            );
        }
    }
}
