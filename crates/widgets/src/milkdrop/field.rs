use num_traits::ToPrimitive;

use crate::spectrum::Spectrum;

pub(crate) fn usize_to_f32(value: usize) -> f32 {
    value.to_f32().unwrap_or(f32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BandRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BandSplit {
    pub(crate) low: BandRange,
    pub(crate) mid: BandRange,
    pub(crate) high: BandRange,
}

impl Default for BandSplit {
    fn default() -> Self {
        Self {
            low: BandRange { start: 0, end: 3 },
            mid: BandRange { start: 3, end: 9 },
            high: BandRange { start: 9, end: 16 },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BandLevels {
    pub(crate) bass: f32,
    pub(crate) mid: f32,
    pub(crate) treble: f32,
}

pub(crate) fn band_mean(bands: &Spectrum, range: BandRange) -> f32 {
    let slice = bands.get(range.start..range.end).unwrap_or_default();
    if slice.is_empty() {
        return 0.0;
    }
    slice.iter().sum::<f32>() / usize_to_f32(slice.len())
}

pub(crate) fn band_levels(bands: &Spectrum, split: &BandSplit) -> BandLevels {
    BandLevels {
        bass: band_mean(bands, split.low),
        mid: band_mean(bands, split.mid),
        treble: band_mean(bands, split.high),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PresetTable([Preset; 3]);

impl Default for PresetTable {
    fn default() -> Self {
        Self([
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
        ])
    }
}

pub(crate) fn preset_for_seed(seed: u64) -> Preset {
    let presets = PresetTable::default().0;
    let index = usize::try_from(seed % presets.len() as u64).unwrap_or(0);
    presets.get(index).copied().unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MilkdropCoefficients {
    pub(crate) decay: f32,
    pub(crate) zoom_gain: f32,
    pub(crate) rotation_gain: f32,
    pub(crate) core_radius: f32,
    pub(crate) core_gain: f32,
    pub(crate) spark_count: u32,
    pub(crate) aspect_x: f32,
}

impl Default for MilkdropCoefficients {
    fn default() -> Self {
        Self {
            decay: 0.85,
            zoom_gain: 0.35,
            rotation_gain: 0.5,
            core_radius: 1.2,
            core_gain: 1.8,
            spark_count: 3,
            aspect_x: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MilkdropGlyphs {
    pub(crate) ramp: [&'static str; 5],
    pub(crate) fallback: &'static str,
}

impl Default for MilkdropGlyphs {
    fn default() -> Self {
        Self {
            ramp: [" ", "░", "▒", "▓", "█"],
            fallback: "█",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MilkdropColorBands {
    pub(crate) mid: f32,
    pub(crate) high: f32,
}

impl Default for MilkdropColorBands {
    fn default() -> Self {
        Self {
            mid: 0.35,
            high: 0.7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellPosition {
    pub(crate) column: usize,
    pub(crate) row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldDimensions {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FieldCenter {
    pub(crate) column: f32,
    pub(crate) row: f32,
}

pub(crate) fn field_center(dimensions: FieldDimensions) -> FieldCenter {
    FieldCenter {
        column: usize_to_f32(dimensions.width.saturating_sub(1)) / 2.0,
        row: usize_to_f32(dimensions.height.saturating_sub(1)) / 2.0,
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
    dimensions: FieldDimensions,
    source: PlaneOffset,
) -> f32 {
    let last_column = usize_to_f32(dimensions.width.saturating_sub(1));
    let last_row = usize_to_f32(dimensions.height.saturating_sub(1));
    let out_of_bounds = source.column < 0.0
        || source.row < 0.0
        || source.column > last_column
        || source.row > last_row;
    if out_of_bounds {
        return 0.0;
    }

    let column_low = raster::floor_usize(source.column);
    let row_low = raster::floor_usize(source.row);
    let column_high = (column_low + 1).min(dimensions.width.saturating_sub(1));
    let row_high = (row_low + 1).min(dimensions.height.saturating_sub(1));
    let column_fraction = source.column - usize_to_f32(column_low);
    let row_fraction = source.row - usize_to_f32(row_low);

    let at = |row: usize, column: usize| {
        cells
            .get(row * dimensions.width + column)
            .copied()
            .unwrap_or(0.0)
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

pub(crate) fn inject(
    cells: &mut [f32],
    dimensions: FieldDimensions,
    injection: &Injection,
) {
    for (row, cells_row) in row_chunks_mut(cells, dimensions).enumerate() {
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

    let width = dimensions.width.max(1) as u64;
    let height = dimensions.height.max(1) as u64;
    let mut state = (injection.seed ^ injection.tick) | 1;
    for _ in 0..injection.spark_count {
        state = xorshift64(state);
        let column = usize::try_from(state % width).unwrap_or(0);
        state = xorshift64(state);
        let row = usize::try_from(state % height).unwrap_or(0);
        let index = row * dimensions.width + column;
        if let Some(cell) = cells.get_mut(index) {
            *cell = cell.max(injection.treble);
        }
    }
}

pub(crate) fn mirror_horizontal_into(
    source: &[f32],
    dest: &mut [f32],
    dimensions: FieldDimensions,
) {
    for (dest_row, source_row) in
        row_chunks_mut(dest, dimensions).zip(row_chunks(source, dimensions))
    {
        write_mirrored_row(dest_row, source_row, dimensions.width);
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
    dimensions: FieldDimensions,
) -> impl Iterator<Item = &[f32]> {
    cells
        .chunks_exact(dimensions.width.max(1))
        .take(dimensions.height)
}

pub(crate) fn row_chunks_mut(
    cells: &mut [f32],
    dimensions: FieldDimensions,
) -> impl Iterator<Item = &mut [f32]> {
    cells
        .chunks_exact_mut(dimensions.width.max(1))
        .take(dimensions.height)
}

pub(crate) fn kaleidoscope_quadrants_into(
    source: &[f32],
    dest: &mut [f32],
    dimensions: FieldDimensions,
) {
    for (row, dest_row) in row_chunks_mut(dest, dimensions).enumerate() {
        let source_row = row_chunks(source, dimensions)
            .nth(mirrored(row, dimensions.height))
            .unwrap_or_default();
        write_mirrored_row(dest_row, source_row, dimensions.width);
    }
}
