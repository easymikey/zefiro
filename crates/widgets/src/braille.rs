use num_traits::ToPrimitive;

use crate::pixels::floor;

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

pub(crate) fn dot_coord(columns: u32) -> u16 {
    u16::try_from(columns).unwrap_or(u16::MAX)
}

const HALF_STEP_BIAS: f32 = 0.001;

fn scaled_dots(level: f32, max_dots: u32) -> u32 {
    let max_dots = max_dots.to_f32().unwrap_or(f32::MAX);
    floor::<u32>(level * max_dots + 0.5 - HALF_STEP_BIAS)
}

#[cfg(test)]
mod rounding_tests {
    use crate::braille::scaled_dots;

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

#[derive(Debug, Default)]
pub(crate) struct BrailleCanvas {
    width_cells: u16,
    height_cells: u16,
    cells: Vec<u8>,
}

impl BrailleCanvas {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new(width_cells: u16, height_cells: u16) -> Self {
        Self {
            width_cells,
            height_cells,
            cells: vec![0u8; usize::from(width_cells) * usize::from(height_cells)],
        }
    }

    fn resize_and_clear(&mut self, width_cells: u16, height_cells: u16) {
        let length = usize::from(width_cells) * usize::from(height_cells);
        if self.width_cells == width_cells && self.height_cells == height_cells {
            self.cells.iter_mut().for_each(|dot| *dot = 0);
        } else {
            self.width_cells = width_cells;
            self.height_cells = height_cells;
            self.cells.clear();
            self.cells.resize(length, 0);
        }
    }

    pub(crate) fn set(&mut self, x: u16, y: u16) {
        let dot_width = self.width_cells * 2;
        let dot_height = self.height_cells * 4;
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

        let index =
            usize::from(cell_y) * usize::from(self.width_cells) + usize::from(cell_x);
        if let Some(slot) = self.cells.get_mut(index) {
            *slot |= 1 << bit;
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn rows(&self) -> Vec<String> {
        (0..usize::from(self.height_cells))
            .map(|row| {
                let start = row * usize::from(self.width_cells);
                let end = start + usize::from(self.width_cells);
                self.cells
                    .get(start..end)
                    .unwrap_or(&[])
                    .iter()
                    .map(|&mask| glyph_for_mask(mask))
                    .collect()
            })
            .collect()
    }

    fn rows_into(&self, out: &mut Vec<String>) {
        let height = usize::from(self.height_cells);
        let width = usize::from(self.width_cells);
        out.truncate(height);
        while out.len() < height {
            out.push(String::new());
        }
        for (row, out_row) in out.iter_mut().enumerate() {
            let start = row * width;
            let end = start + width;
            out_row.clear();
            for &mask in self.cells.get(start..end).unwrap_or(&[]) {
                out_row.push(glyph_for_mask(mask));
            }
        }
    }
}

fn fill_meter(canvas: &mut BrailleCanvas, fill: &MeterFill<'_>) {
    let MeterFill {
        size,
        levels,
        max_dots,
    } = *fill;
    if levels.is_empty() || size.width == 0 {
        return;
    }
    let base = u32::from(size.height) * 4;
    let cap = max_dots.clamp(1, base);
    let floor = 1;
    let total_dot_cols = u32::from(size.width) * 2;
    let count = small_len_u32(levels.len());
    for (step, &level) in levels.iter().enumerate() {
        let step = small_len_u32(step);
        let start = step * total_dot_cols / count;
        let end = ((step + 1) * total_dot_cols / count).max(start + 1);
        let fill_end = if end > start + 1 { end - 1 } else { end };
        let level = level.clamp(0.0, 1.0);
        let filled = scaled_dots(level, cap).max(floor);
        for x in start..fill_end {
            for k in 0..filled {
                let y = base - 1 - k;
                canvas.set(dot_coord(x), dot_coord(y));
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct BrailleBuffers {
    canvas: BrailleCanvas,
    rows: Vec<String>,
}

impl BrailleBuffers {
    pub(crate) fn render_meter(&mut self, fill: &MeterFill<'_>) -> &[String] {
        self.canvas
            .resize_and_clear(fill.size.width, fill.size.height);
        fill_meter(&mut self.canvas, fill);
        self.canvas.rows_into(&mut self.rows);
        &self.rows
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
    use crate::braille::BrailleCanvas;

    #[test]
    fn empty_canvas_is_blank() {
        let canvas = BrailleCanvas::new(3, 2);
        let rows = canvas.rows();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.chars().count(), 3);
            for ch in row.chars() {
                assert_eq!(ch, '\u{2800}');
            }
        }
    }

    #[test]
    fn single_dot_maps_to_correct_cell_bit() {
        let mut top_left = BrailleCanvas::new(1, 1);
        top_left.set(0, 0);
        assert_eq!(
            top_left.rows().first().map(String::as_str),
            Some("\u{2801}")
        );

        let mut bottom_right_column = BrailleCanvas::new(1, 1);
        bottom_right_column.set(1, 3);
        assert_eq!(
            bottom_right_column.rows().first().map(String::as_str),
            Some("\u{2880}")
        );

        let mut bottom_left_column = BrailleCanvas::new(1, 1);
        bottom_left_column.set(0, 3);
        assert_eq!(
            bottom_left_column.rows().first().map(String::as_str),
            Some("\u{2840}")
        );

        bottom_left_column.set(100, 100);
        assert_eq!(
            bottom_left_column.rows().first().map(String::as_str),
            Some("\u{2840}")
        );
    }

    #[test]
    fn full_column_is_full_glyph() {
        let mut canvas = BrailleCanvas::new(1, 1);
        for y in 0..4 {
            canvas.set(0, y);
            canvas.set(1, y);
        }
        assert_eq!(canvas.rows().first().map(String::as_str), Some("\u{28FF}"));
    }
}
