use config::CoverStyle;

use crate::vinyl::{VinylLayout, canvas_aspect_ratio};

#[must_use]
pub fn cover_aspect_ratio(style: CoverStyle) -> f32 {
    match style {
        CoverStyle::Plain | CoverStyle::Off | CoverStyle::Milkdrop => 1.0,
        CoverStyle::Vinyl => canvas_aspect_ratio(&VinylLayout::default()),
    }
}

#[cfg(test)]
mod tests {
    use config::CoverStyle;

    use crate::{
        cover::cover_aspect_ratio,
        vinyl::{VinylLayout, canvas_aspect_ratio},
    };

    #[test]
    fn a_square_cover_has_a_square_aspect_ratio() {
        for style in [CoverStyle::Plain, CoverStyle::Off, CoverStyle::Milkdrop] {
            assert_eq!(cover_aspect_ratio(style), 1.0);
        }
    }

    #[test]
    fn a_vinyl_cover_uses_the_vinyl_canvas_aspect_ratio() {
        assert_eq!(
            cover_aspect_ratio(CoverStyle::Vinyl),
            canvas_aspect_ratio(&VinylLayout::default())
        );
    }
}
