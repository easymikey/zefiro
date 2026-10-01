use ratatui::layout::Rect;

use crate::{pixels::cover::CoverWash, wash_reveal};

#[must_use]
pub fn column_reveal(
    rect: Rect,
    cell_width_px: u16,
    wash: CoverWash,
) -> Option<impl Fn(u32) -> f32> {
    let CoverWash::Running {
        progress,
        screen_width,
    } = wash
    else {
        return None;
    };
    let cell_width_px = u32::from(cell_width_px.max(1));
    Some(move |x_px: u32| {
        let offset = u16::try_from(x_px / cell_width_px).unwrap_or(u16::MAX);
        wash_reveal(progress, rect.x.saturating_add(offset), screen_width)
    })
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use ratatui::layout::Rect;

    use crate::pixels::cover::{
        CoverWash,
        crossfade::blend_by_column,
        wash::column_reveal,
    };

    const OLD_PIXEL: Rgba<u8> = Rgba([200, 0, 0, 255]);
    const NEW_PIXEL: Rgba<u8> = Rgba([0, 200, 0, 255]);

    fn filled(width: u32, pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(width, 2, pixel)
    }

    fn wash_frame(width: u32, rect: Rect, wash: CoverWash) -> RgbaImage {
        let reveal =
            column_reveal(rect, 1, wash).expect("a running wash reveals columns");
        blend_by_column(&filled(width, OLD_PIXEL), &filled(width, NEW_PIXEL), reveal)
    }

    #[test]
    fn the_left_columns_show_the_new_theme_and_the_right_columns_the_old() {
        let frame = wash_frame(
            40,
            Rect::new(80, 0, 40, 4),
            CoverWash::Running {
                progress: 0.5,
                screen_width: 200,
            },
        );

        let left = frame.get_pixel(0, 0);
        let right = frame.get_pixel(39, 0);
        assert_eq!(*left, NEW_PIXEL, "the leftmost column must reveal first");
        assert_eq!(
            *right, OLD_PIXEL,
            "the rightmost column must stay old longest"
        );
    }

    #[test]
    fn a_finished_wash_shows_the_new_theme_everywhere() {
        let frame = wash_frame(
            20,
            Rect::new(0, 0, 20, 2),
            CoverWash::Running {
                progress: 1.0,
                screen_width: 20,
            },
        );

        assert!(frame.pixels().all(|pixel| *pixel == NEW_PIXEL));
    }

    #[test]
    fn an_idle_wash_reveals_nothing() {
        assert!(column_reveal(Rect::default(), 1, CoverWash::Idle).is_none());
    }
}
