use image::RgbaImage;

use crate::{
    numeric::{dimension_f32, dimension_u32},
    vinyl::VinylFrameStyle,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylLayout {
    pub disc_fraction: f32,
    pub slide_fraction: f32,
    pub groove_count: u32,
    pub groove_spacing: f32,
    pub label_radius_fraction: f32,
    pub spindle_radius_fraction: f32,
    pub border_width: f32,
    pub label_border_width: f32,
    pub sleeve_padding: f32,
    pub corner_radius: f32,
    pub shadow_offset: f32,
    pub groove_alpha: u8,
    pub shadow_alpha: u8,
}

impl Default for VinylLayout {
    fn default() -> Self {
        Self {
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
        }
    }
}

#[must_use]
pub fn canvas_aspect_ratio(layout: &VinylLayout) -> f32 {
    1.0 + layout.slide_fraction * layout.disc_fraction
        + layout.shadow_offset * shadow_horizontal_reach_fraction()
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
pub(crate) struct StrokeStyle {
    pub(crate) color: tiny_skia::Color,
    pub(crate) width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowStyle {
    pub(crate) color: config::Rgb,
    pub(crate) offset: f32,
    pub(crate) peak_alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RecordGeometry {
    pub(crate) record: Disc,
    pub(crate) label_radius: f32,
}

pub(crate) struct LabelArt<'a> {
    pub(crate) art: &'a RgbaImage,
    pub(crate) disc: Disc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SleevePlacement {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) side: f32,
}

pub(crate) struct VinylGeometry {
    pub(crate) size: f32,
    pub(crate) disc_diameter: f32,
    pub(crate) peek: f32,
    pub(crate) shadow_margin: f32,
}

impl VinylGeometry {
    pub(crate) fn new(size_px: u32, layout: &VinylLayout) -> Self {
        let size = dimension_f32(size_px);
        let disc_diameter = layout.disc_fraction * size;
        let peek = layout.slide_fraction * disc_diameter;
        let shadow_offset_px = layout.shadow_offset * size;
        let shadow_margin = shadow_offset_px * shadow_horizontal_reach_fraction();
        Self {
            size,
            disc_diameter,
            peek,
            shadow_margin,
        }
    }

    pub(crate) fn canvas_width_px(&self, size_px: u32) -> u32 {
        size_px.saturating_add(dimension_u32((self.peek + self.shadow_margin).ceil()))
    }
}

pub(crate) struct CanvasSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn canvas_size(style: &VinylFrameStyle) -> CanvasSize {
    let size_px = style.size_px.max(1);
    let geometry = VinylGeometry::new(size_px, &style.layout);
    CanvasSize {
        width: geometry.canvas_width_px(size_px),
        height: size_px,
    }
}

pub(crate) fn record_disc(geometry: &VinylGeometry) -> Disc {
    let record_radius = geometry.disc_diameter / 2.0;
    Disc {
        center_x: geometry.size + geometry.peek - record_radius,
        center_y: geometry.size / 2.0,
        radius: record_radius,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        numeric::dimension_f32,
        vinyl::geometry::{
            VinylGeometry,
            VinylLayout,
            record_disc,
            shadow_horizontal_reach_fraction,
        },
    };

    #[test]
    fn disc_and_shadow_fit_inside_the_canvas() {
        let layout = VinylLayout::default();
        for size_px in [96_u32, 160_u32] {
            let geometry = VinylGeometry::new(size_px, &layout);
            let disc = record_disc(&geometry);
            let shadow_offset_px = layout.shadow_offset * geometry.size;
            let shadow_right_edge_px = disc.center_x
                + disc.radius
                + shadow_offset_px * shadow_horizontal_reach_fraction();
            let canvas_width_px = dimension_f32(geometry.canvas_width_px(size_px));
            assert!(
                shadow_right_edge_px <= canvas_width_px,
                "disc+shadow right edge {shadow_right_edge_px} exceeds canvas width \
                 {canvas_width_px} at size_px={size_px}"
            );
        }
    }
}
