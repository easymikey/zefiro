use image::RgbaImage;
use ratatui::layout::Rect;
use widgets::wash_reveal;

use crate::pixels::cover::crossfade::{CLEAR, mixed};

/// A pixel-image theme wash: blends `old` into `new` column by column, using
/// the same left-to-right reveal the cell-based screen wash uses, so a cover
/// pixmap tracks the curtain instead of flipping straight to the new theme.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WashFrame<'a> {
    pub(crate) old: &'a RgbaImage,
    pub(crate) new: &'a RgbaImage,
    pub(crate) rect: Rect,
    pub(crate) cell_width_px: u16,
    pub(crate) progress: f32,
    pub(crate) screen_width: u16,
}

#[must_use]
pub(crate) fn wash_frame(input: WashFrame<'_>) -> RgbaImage {
    let WashFrame {
        old,
        new,
        rect,
        cell_width_px,
        progress,
        screen_width,
    } = input;
    let cell_width_px = u32::from(cell_width_px.max(1));
    RgbaImage::from_fn(new.width(), new.height(), |x_px, y_px| {
        let column = rect.x.saturating_add(column_offset(x_px, cell_width_px));
        let reveal = wash_reveal(progress, column, screen_width);
        let front = new.get_pixel_checked(x_px, y_px).copied().unwrap_or(CLEAR);
        old.get_pixel_checked(x_px, y_px)
            .map_or(front, |back| mixed(*back, front, reveal))
    })
}

fn column_offset(x_px: u32, cell_width_px: u32) -> u16 {
    u16::try_from(x_px / cell_width_px).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use ratatui::layout::Rect;

    use crate::pixels::cover::wash::{WashFrame, wash_frame};

    const OLD_PIXEL: Rgba<u8> = Rgba([200, 0, 0, 255]);
    const NEW_PIXEL: Rgba<u8> = Rgba([0, 200, 0, 255]);

    fn filled(width: u32, pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(width, 2, pixel)
    }

    #[test]
    fn the_left_columns_show_the_new_theme_and_the_right_columns_the_old() {
        let old = filled(40, OLD_PIXEL);
        let new = filled(40, NEW_PIXEL);
        let frame = wash_frame(WashFrame {
            old: &old,
            new: &new,
            rect: Rect::new(80, 0, 40, 4),
            cell_width_px: 1,
            progress: 0.5,
            screen_width: 200,
        });

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
        let old = filled(20, OLD_PIXEL);
        let new = filled(20, NEW_PIXEL);
        let frame = wash_frame(WashFrame {
            old: &old,
            new: &new,
            rect: Rect::new(0, 0, 20, 2),
            cell_width_px: 1,
            progress: 1.0,
            screen_width: 20,
        });

        assert!(frame.pixels().all(|pixel| *pixel == NEW_PIXEL));
    }
}
