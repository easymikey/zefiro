use num_traits::ToPrimitive;

use crate::spectrum::Spectrum;

pub(crate) fn usize_to_f32(count: usize) -> f32 {
    count.to_f32().unwrap_or(f32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BandRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

const LOW_BAND: BandRange = BandRange { start: 0, end: 3 };
const MID_BAND: BandRange = BandRange { start: 3, end: 9 };
const HIGH_BAND: BandRange = BandRange { start: 9, end: 16 };

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BandLevels {
    pub(crate) bass: f32,
    pub(crate) mid: f32,
    pub(crate) treble: f32,
}

pub(crate) fn band_mean(bands: &Spectrum, range: BandRange) -> f32 {
    let slice = bands.get(range.start..range.end).unwrap_or(&[]);
    if slice.is_empty() {
        return 0.0;
    }
    slice.iter().sum::<f32>() / usize_to_f32(slice.len())
}

pub(crate) fn band_levels(bands: &Spectrum) -> BandLevels {
    BandLevels {
        bass: band_mean(bands, LOW_BAND),
        mid: band_mean(bands, MID_BAND),
        treble: band_mean(bands, HIGH_BAND),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mirror {
    None,
    Horizontal,
    Kaleido4,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Preset {
    pub(crate) base_zoom: f32,
    pub(crate) base_rotation: f32,
    pub(crate) mirror: Mirror,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            base_zoom: 1.0,
            base_rotation: 0.0,
            mirror: Mirror::None,
        }
    }
}

const PRESETS: [Preset; 3] = [
    Preset {
        base_zoom: 1.12,
        base_rotation: 0.04,
        mirror: Mirror::None,
    },
    Preset {
        base_zoom: 0.94,
        base_rotation: -0.05,
        mirror: Mirror::Horizontal,
    },
    Preset {
        base_zoom: 1.18,
        base_rotation: 0.07,
        mirror: Mirror::Kaleido4,
    },
];

pub(crate) fn preset_for_seed(seed: u64) -> Preset {
    let index = u64::try_from(PRESETS.len()).map_or(0, |len| seed % len);
    let index = usize::try_from(index).unwrap_or(0);
    PRESETS.get(index).copied().unwrap_or_else(Preset::default)
}

pub(crate) const DECAY: f32 = 0.85;
pub(crate) const ZOOM_GAIN: f32 = 0.35;
pub(crate) const ROTATION_GAIN: f32 = 0.5;
pub(crate) const CORE_RADIUS: f32 = 1.2;
pub(crate) const CORE_GAIN: f32 = 1.8;
pub(crate) const SPARK_COUNT: u32 = 3;
pub(crate) const ASPECT_X: f32 = 0.5;

pub(crate) const RAMP: [&str; 5] = [" ", "░", "▒", "▓", "█"];
pub(crate) const RAMP_FALLBACK: &str = "█";

pub(crate) const COLOR_BAND_MID: f32 = 0.35;
pub(crate) const COLOR_BAND_HIGH: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellPosition {
    pub(crate) column: usize,
    pub(crate) row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldSize {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FieldCenter {
    pub(crate) column: f32,
    pub(crate) row: f32,
}

pub(crate) fn field_center(size: FieldSize) -> FieldCenter {
    FieldCenter {
        column: usize_to_f32(size.width.saturating_sub(1)) / 2.0,
        row: usize_to_f32(size.height.saturating_sub(1)) / 2.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaneOffset {
    pub(crate) column: f32,
    pub(crate) row: f32,
}

pub(crate) fn physical_offset(
    position: CellPosition,
    center: FieldCenter,
    aspect_x: f32,
) -> PlaneOffset {
    PlaneOffset {
        column: (usize_to_f32(position.column) - center.column) * aspect_x,
        row: usize_to_f32(position.row) - center.row,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Warp {
    pub(crate) center: FieldCenter,
    pub(crate) zoom: f32,
    pub(crate) rotation: f32,
    pub(crate) aspect_x: f32,
}

pub(crate) fn warp_source(position: CellPosition, warp: &Warp) -> PlaneOffset {
    let offset = physical_offset(position, warp.center, warp.aspect_x);
    let scaled = PlaneOffset {
        column: offset.column / warp.zoom,
        row: offset.row / warp.zoom,
    };
    let angle = -warp.rotation;
    let (sin, cos) = angle.sin_cos();
    let rotated = PlaneOffset {
        column: scaled.column * cos - scaled.row * sin,
        row: scaled.column * sin + scaled.row * cos,
    };
    PlaneOffset {
        column: warp.center.column + rotated.column / warp.aspect_x,
        row: warp.center.row + rotated.row,
    }
}

pub(crate) fn bilinear_sample(
    cells: &[f32],
    size: FieldSize,
    source: PlaneOffset,
) -> f32 {
    let last_column = usize_to_f32(size.width.saturating_sub(1));
    let last_row = usize_to_f32(size.height.saturating_sub(1));
    let out_of_bounds = source.column < 0.0
        || source.row < 0.0
        || source.column > last_column
        || source.row > last_row;
    if out_of_bounds {
        return 0.0;
    }

    let column_low = crate::pixels::floor::<usize>(source.column);
    let row_low = crate::pixels::floor::<usize>(source.row);
    let column_high = (column_low + 1).min(size.width.saturating_sub(1));
    let row_high = (row_low + 1).min(size.height.saturating_sub(1));
    let column_fraction = source.column - usize_to_f32(column_low);
    let row_fraction = source.row - usize_to_f32(row_low);

    let at = |row: usize, column: usize| {
        cells.get(row * size.width + column).copied().unwrap_or(0.0)
    };
    let top = lerp(
        at(row_low, column_low),
        at(row_low, column_high),
        column_fraction,
    );
    let bottom = lerp(
        at(row_high, column_low),
        at(row_high, column_high),
        column_fraction,
    );
    lerp(top, bottom, row_fraction)
}

pub(crate) fn lerp(start: f32, end: f32, fraction: f32) -> f32 {
    start + (end - start) * fraction
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Injection {
    pub(crate) center: FieldCenter,
    pub(crate) aspect_x: f32,
    pub(crate) core_radius: f32,
    pub(crate) treble: f32,
    pub(crate) spark_count: u32,
    pub(crate) seed: u64,
    pub(crate) tick: u64,
}

pub(crate) fn xorshift64(state: u64) -> u64 {
    let mut value = state;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    value
}

pub(crate) fn inject(cells: &mut [f32], size: FieldSize, injection: &Injection) {
    for (row, cells_row) in row_chunks_mut(cells, size).enumerate() {
        for (column, cell) in cells_row.iter_mut().enumerate() {
            let position = CellPosition { column, row };
            let offset =
                physical_offset(position, injection.center, injection.aspect_x);
            let distance =
                (offset.column * offset.column + offset.row * offset.row).sqrt();
            if distance <= injection.core_radius {
                *cell = cell.max(1.0);
            }
        }
    }

    let width = u64::try_from(size.width.max(1)).unwrap_or(1);
    let height = u64::try_from(size.height.max(1)).unwrap_or(1);
    let mut state = (injection.seed ^ injection.tick) | 1;
    for _ in 0..injection.spark_count {
        state = xorshift64(state);
        let column = usize::try_from(state % width).unwrap_or(0);
        state = xorshift64(state);
        let row = usize::try_from(state % height).unwrap_or(0);
        let index = row * size.width + column;
        if let Some(cell) = cells.get_mut(index) {
            *cell = cell.max(injection.treble);
        }
    }
}

pub(crate) fn mirror_horizontal_into(
    source: &[f32],
    dest: &mut [f32],
    size: FieldSize,
) {
    for (dest_row, source_row) in
        row_chunks_mut(dest, size).zip(row_chunks(source, size))
    {
        write_mirrored_row(dest_row, source_row, size.width);
    }
}

pub(crate) fn write_mirrored_row(
    dest_row: &mut [f32],
    source_row: &[f32],
    width: usize,
) {
    for (column, cell) in dest_row.iter_mut().enumerate() {
        *cell = source_row
            .get(mirrored(column, width))
            .copied()
            .unwrap_or(0.0);
    }
}

pub(crate) fn mirrored(position: usize, extent: usize) -> usize {
    if position < extent / 2 {
        position
    } else {
        extent.saturating_sub(1).saturating_sub(position)
    }
}

pub(crate) fn row_chunks(
    cells: &[f32],
    size: FieldSize,
) -> impl Iterator<Item = &[f32]> {
    cells.chunks_exact(size.width.max(1)).take(size.height)
}

pub(crate) fn row_chunks_mut(
    cells: &mut [f32],
    size: FieldSize,
) -> impl Iterator<Item = &mut [f32]> {
    cells.chunks_exact_mut(size.width.max(1)).take(size.height)
}

pub(crate) fn kaleidoscope_quadrants_into(
    source: &[f32],
    dest: &mut [f32],
    size: FieldSize,
) {
    for (row, dest_row) in row_chunks_mut(dest, size).enumerate() {
        let source_row = row_chunks(source, size)
            .nth(mirrored(row, size.height))
            .unwrap_or(&[]);
        write_mirrored_row(dest_row, source_row, size.width);
    }
}
