use kernel::domain::geometry::{Cells, Pixels};
use ratatui::layout::Rect;

use crate::{
    animation::catalogue::wash_reveal,
    pixels::{cover::CoverWash, numeric::small_count_u16},
};

#[must_use]
pub fn cover_wash(progress: Option<f32>, screen_width: Cells) -> CoverWash {
    progress.map_or(CoverWash::Idle, |progress| CoverWash::Running {
        progress,
        screen_width,
    })
}

#[must_use]
pub(crate) fn column_reveal(
    rect: Rect,
    cell_width: Pixels,
    wash: CoverWash,
) -> Option<impl Fn(u32) -> f32> {
    let CoverWash::Running {
        progress,
        screen_width,
    } = wash
    else {
        return None;
    };
    let cell_width = cell_width.0.max(1);
    Some(move |x: u32| {
        let offset = small_count_u16(x / cell_width);
        wash_reveal(progress, rect.x.saturating_add(offset), screen_width.0)
    })
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use kernel::domain::geometry::{Cells, Pixels};
    use ratatui::layout::Rect;

    use crate::pixels::cover::{
        CoverWash,
        crossfade::blend_by_column,
        wash::{column_reveal, cover_wash},
    };

    const OLD_PIXEL: Rgba<u8> = Rgba([200, 0, 0, 255]);
    const NEW_PIXEL: Rgba<u8> = Rgba([0, 200, 0, 255]);

    fn filled(width: u32, pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(width, 2, pixel)
    }

    fn wash_frame(width: u32, rect: Rect, wash: CoverWash) -> RgbaImage {
        let reveal = column_reveal(rect, Pixels(1), wash)
            .expect("a running wash reveals columns");
        blend_by_column(&filled(width, OLD_PIXEL), &filled(width, NEW_PIXEL), reveal)
    }

    #[test]
    fn the_left_columns_show_the_new_theme_and_the_right_columns_the_old() {
        let frame = wash_frame(
            40,
            Rect::new(80, 0, 40, 4),
            CoverWash::Running {
                progress: 0.5,
                screen_width: Cells(200),
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
                screen_width: Cells(20),
            },
        );

        assert!(frame.pixels().all(|pixel| *pixel == NEW_PIXEL));
    }

    #[test]
    fn an_idle_wash_reveals_nothing() {
        assert!(column_reveal(Rect::default(), Pixels(1), CoverWash::Idle).is_none());
    }

    #[test]
    fn no_wash_progress_is_an_idle_wash() {
        assert_eq!(cover_wash(None, Cells(80)), CoverWash::Idle);
    }

    #[test]
    fn a_wash_progress_carries_the_screen_width_along() {
        assert_eq!(
            cover_wash(Some(0.4), Cells(80)),
            CoverWash::Running {
                progress: 0.4,
                screen_width: Cells(80),
            }
        );
    }
}
