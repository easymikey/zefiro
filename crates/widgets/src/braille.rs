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
    use rstest::rstest;

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

    #[rstest]
    #[case::empty(0.0, 8, 0)]
    #[case::full(1.0, 8, 8)]
    #[case::an_exact_half_step_rounds_down(4.5 / 8.0, 8, 4)]
    #[case::float_noise_below_a_half_step_rounds_down(4.5 / 8.0 - 1e-6, 8, 4)]
    #[case::float_noise_above_a_half_step_rounds_down(4.5 / 8.0 + 1e-6, 8, 4)]
    #[case::past_a_half_step_rounds_up(0.6, 8, 5)]
    fn scaled_dots_rounds_to_the_nearest_dot_and_a_half_step_down(
        #[case] fraction: f32,
        #[case] max_dots: u32,
        #[case] expected: u32,
    ) {
        assert_eq!(scaled_dots(fraction, max_dots), expected);
    }

    #[rstest]
    #[case::first_dot(0, 0, [0b0000_0001, 0, 0, 0])]
    #[case::last_dot(3, 7, [0, 0, 0, 0b1000_0000])]
    #[case::past_the_right_edge(4, 0, [0; 4])]
    #[case::past_the_bottom_edge(0, 8, [0; 4])]
    fn set_lights_one_dot_inside_the_canvas_and_none_outside(
        #[case] x: u16,
        #[case] y: u16,
        #[case] cells: [u8; 4],
    ) {
        let mut braille_canvas = BrailleCanvas::new(Cells(2), Cells(2));
        braille_canvas.set(x, y);
        assert_eq!(braille_canvas.cells, cells);
    }

    #[rstest]
    #[case::one_level_fills_all_but_the_last_dot_column(2, &[0.5], &[0xE4, 0x44])]
    #[case::each_level_leaves_a_gap_before_the_next(
        4,
        &[1.0, 1.0],
        &[0xFF, 0x47, 0xFF, 0x47]
    )]
    fn a_meter_fills_each_level_from_the_bottom_across_its_share_of_columns(
        #[case] width: u16,
        #[case] levels: &[f32],
        #[case] cells: &[u8],
    ) {
        let braille_canvas = BrailleCanvas::from(&MeterFill {
            size: CanvasSize { width, height: 1 },
            levels,
            max_dots: 4,
        });
        assert_eq!(braille_canvas.cells, cells);
    }
}
