use image::RgbaImage;
use kernel::domain::geometry::Pixels;
use tiny_skia::{ColorU8, FillRule, Mask, Path, Pixmap, PixmapPaint, Transform};

use crate::pixels::{
    numeric::{dimension_f32, round},
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
    round::<u32>(size - pad * 2.0).max(1)
}

fn label_diameter(canvas_side: Pixels) -> u32 {
    round::<u32>(VinylGeometry::new(canvas_side).label_radius * 2.0).max(1)
}

#[derive(Debug)]
pub(crate) struct VinylArt {
    pub(crate) sleeve: Pixmap,
    pub(crate) label: Pixmap,
}

#[must_use]
pub(crate) fn prepare_art(image: &RgbaImage, canvas_side: Pixels) -> Option<VinylArt> {
    let premultiplied = |side: u32| {
        let resized = cover_crop_resize(image, side, side);
        let mut pixmap = Pixmap::new(side, side)?;
        for (target, source) in pixmap.pixels_mut().iter_mut().zip(resized.pixels()) {
            let [red, green, blue, alpha] = source.0;
            *target = ColorU8::from_rgba(red, green, blue, alpha).premultiply();
        }
        Some(pixmap)
    };
    Some(VinylArt {
        sleeve: premultiplied(sleeve_inset_side(canvas_side))?,
        label: premultiplied(label_diameter(canvas_side))?,
    })
}

pub(crate) struct ArtClip {
    path: Path,
    x: f32,
    y: f32,
}

impl ArtClip {
    pub(crate) fn rounded_rect(rect: RoundedRect) -> Option<Self> {
        Some(Self {
            path: rounded_rect_path(rect)?,
            x: rect.x.round(),
            y: rect.y.round(),
        })
    }

    pub(crate) fn circle(disc: Disc) -> Option<Self> {
        let diameter = dimension_f32(round::<u32>(disc.radius * 2.0).max(1));
        Some(Self {
            path: circle_path(disc)?,
            x: disc.center_x - diameter / 2.0,
            y: disc.center_y - diameter / 2.0,
        })
    }
}

pub(crate) fn paint_art_clipped(pixmap: &mut Pixmap, art: &Pixmap, clip: &ArtClip) {
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    mask.fill_path(&clip.path, FillRule::Winding, true, Transform::identity());
    pixmap.draw_pixmap(
        0,
        0,
        art.as_ref(),
        &PixmapPaint::default(),
        Transform::from_translate(clip.x, clip.y),
        Some(&mask),
    );
}

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Pixels;
    use rstest::rstest;

    use crate::pixels::vinyl::{
        art::{ArtClip, prepare_art},
        geometry::Disc,
        tests::synthetic_art,
    };

    #[rstest]
    #[case::small_canvas(Pixels(272), 261, 88)]
    #[case::large_canvas(Pixels(1000), 958, 322)]
    #[case::empty_canvas(Pixels(0), 1, 1)]
    fn prepared_art_has_the_sleeve_inset_and_label_diameter_sides(
        #[case] canvas_side: Pixels,
        #[case] sleeve_side: u32,
        #[case] label_side: u32,
    ) {
        let prepared = prepare_art(&synthetic_art(400), canvas_side);
        assert_eq!(
            prepared.as_ref().map(|prepared| (
                prepared.sleeve.width(),
                prepared.sleeve.height(),
                prepared.label.width(),
                prepared.label.height()
            )),
            Some((sleeve_side, sleeve_side, label_side, label_side))
        );
    }

    #[rstest]
    #[case::odd_diameter(Disc { center_x: 100.0, center_y: 50.0, radius: 10.3 }, 89.5, 39.5)]
    #[case::even_diameter(Disc { center_x: 100.0, center_y: 50.0, radius: 8.0 }, 92.0, 42.0)]
    fn a_circle_clip_places_the_art_at_the_rounded_diameter_corner(
        #[case] disc: Disc,
        #[case] x: f32,
        #[case] y: f32,
    ) {
        assert_eq!(
            ArtClip::circle(disc).map(|clip| (clip.x, clip.y)),
            Some((x, y))
        );
    }
}
