use kernel::domain::geometry::Cells;
use ratatui::style::Color;

use crate::{
    pixels::numeric::{dimension_f32, floor, small_count_u16},
    primitive::canvas::Canvas,
};

const BASE: u32 = 0x2800;

const DOT_BITS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];

pub(crate) fn dot_bit(dx: u16, dy: u16) -> Option<u8> {
    DOT_BITS
        .get(usize::from(dy))
        .and_then(|row| row.get(usize::from(dx)))
        .copied()
}

pub(crate) fn glyph_for_mask(mask: u8) -> char {
    char::from_u32(BASE + u32::from(mask)).unwrap_or('\u{2800}')
}

fn small_len_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

const HALF_STEP_BIAS: f32 = 0.001;

fn scaled_dots(fraction: f32, max_dots: u32) -> u32 {
    let max_dots = dimension_f32(max_dots);
    floor::<u32>(fraction * max_dots + 0.5 - HALF_STEP_BIAS)
}

#[derive(Debug, Default)]
pub(crate) struct BrailleCanvas {
    width: Cells,
    height: Cells,
    cells: Vec<u8>,
}

impl BrailleCanvas {
    #[must_use]
    pub(crate) fn new(width: Cells, height: Cells) -> Self {
        Self {
            width,
            height,
            cells: vec![0u8; width.count() * height.count()],
        }
    }

    pub(crate) fn set(&mut self, x: u16, y: u16) {
        let dot_width = self.width.0 * 2;
        let dot_height = self.height.0 * 4;
        if x >= dot_width || y >= dot_height {
            return;
        }

        let cell_x = x / 2;
        let cell_y = y / 4;
        let dx = x % 2;
        let dy = y % 4;

        let Some(bit) = dot_bit(dx, dy) else {
            return;
        };

        let index = usize::from(cell_y) * self.width.count() + usize::from(cell_x);
        if let Some(slot) = self.cells.get_mut(index) {
            *slot |= 1 << bit;
        }
    }

    pub(crate) fn paint(&self, canvas: Canvas<'_>, color_at: impl Fn(u16) -> Color) {
        let Canvas { area, buffer } = canvas;
        let rows = self.cells.chunks(self.width.count().max(1));
        for (y, row) in (0..self.height.0).zip(rows) {
            let color = color_at(y);
            for (x, &mask) in (0..self.width.0).zip(row) {
                if let Some(cell) = buffer.cell_mut((area.x + x, area.y + y)) {
                    cell.set_char(glyph_for_mask(mask)).set_fg(color);
                }
            }
        }
    }
}

impl From<&MeterFill<'_>> for BrailleCanvas {
    fn from(fill: &MeterFill<'_>) -> Self {
        let mut braille_canvas =
            Self::new(Cells(fill.size.width), Cells(fill.size.height));
        fill_meter(&mut braille_canvas, fill);
        braille_canvas
    }
}

fn fill_meter(braille_canvas: &mut BrailleCanvas, fill: &MeterFill<'_>) {
    let MeterFill {
        size,
        levels,
        max_dots,
    } = *fill;
    if levels.is_empty() || size.width == 0 || size.height == 0 {
        return;
    }
    let base = u32::from(size.height) * 4;
    let cap = max_dots.clamp(1, base);
    let total_dot_cols = u32::from(size.width) * 2;
    let count = small_len_u32(levels.len());
    for (step, &level) in levels.iter().enumerate() {
        let step = small_len_u32(step);
        let start = step * total_dot_cols / count;
        let end = ((step + 1) * total_dot_cols / count).max(start + 1);
        let fill_end = if end > start + 1 { end - 1 } else { end };
        let level = level.clamp(0.0, 1.0);
        let filled = scaled_dots(level, cap).max(1);
        for x in start..fill_end {
            for k in 0..filled {
                let y = base - 1 - k;
                braille_canvas.set(small_count_u16(x), small_count_u16(y));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CanvasSize {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MeterFill<'a> {
    pub(crate) size: CanvasSize,
    pub(crate) levels: &'a [f32],
    pub(crate) max_dots: u32,
}

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Cells;
    use ratatui::{buffer::Buffer, layout::Rect, style::Color};

    use crate::{
        braille::{BrailleCanvas, CanvasSize, MeterFill, scaled_dots},
        primitive::canvas::Canvas,
    };

    #[test]
    fn a_meter_with_no_rows_paints_nothing() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(area);
        BrailleCanvas::from(&MeterFill {
            size: CanvasSize {
                width: 4,
                height: 0,
            },
            levels: &[0.5],
            max_dots: 12,
        })
        .paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer, Buffer::empty(area));
    }

    #[test]
    fn empty_canvas_is_blank() {
        let area = Rect::new(0, 0, 3, 2);
        let mut buffer = Buffer::empty(area);
        BrailleCanvas::new(Cells(3), Cells(2)).paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        for cell in buffer.content() {
            assert_eq!(cell.symbol(), "\u{2800}");
        }
    }

    #[test]
    fn single_dot_maps_to_correct_cell_bit() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        let mut top_left_braille_canvas = BrailleCanvas::new(Cells(1), Cells(1));
        top_left_braille_canvas.set(0, 0);
        top_left_braille_canvas.paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "\u{2801}");

        let mut bottom_right_column_braille_canvas =
            BrailleCanvas::new(Cells(1), Cells(1));
        bottom_right_column_braille_canvas.set(1, 3);
        bottom_right_column_braille_canvas.paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "\u{2880}");

        let mut bottom_left_column_braille_canvas =
            BrailleCanvas::new(Cells(1), Cells(1));
        bottom_left_column_braille_canvas.set(0, 3);
        bottom_left_column_braille_canvas.paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "\u{2840}");

        bottom_left_column_braille_canvas.set(100, 100);
        bottom_left_column_braille_canvas.paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "\u{2840}");
    }

    #[test]
    fn full_column_is_full_glyph() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        let mut braille_canvas = BrailleCanvas::new(Cells(1), Cells(1));
        for y in 0..4 {
            braille_canvas.set(0, y);
            braille_canvas.set(1, y);
        }
        braille_canvas.paint(
            Canvas {
                area,
                buffer: &mut buffer,
            },
            |_| Color::Reset,
        );
        assert_eq!(buffer[(0, 0)].symbol(), "\u{28FF}");
    }

    #[test]
    fn scaled_dots_matches_plain_rounding_away_from_a_boundary() {
        assert_eq!(scaled_dots(0.3, 10), 3);
        assert_eq!(scaled_dots(0.76, 10), 8);
    }

    #[test]
    fn scaled_dots_does_not_flip_across_float_noise_at_a_half_step_boundary() {
        let boundary = 4.5 / 8.0;
        assert_eq!(
            scaled_dots(boundary - 1e-6, 8),
            scaled_dots(boundary + 1e-6, 8)
        );
    }
}
