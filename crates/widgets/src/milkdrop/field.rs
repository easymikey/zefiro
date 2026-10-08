use crate::{
    pixels::numeric::{dimension_f32, floor},
    spectrum::Spectrum,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BandRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

const LOW_BAND: BandRange = BandRange { start: 0, end: 3 };
const MID_BAND: BandRange = BandRange { start: 3, end: 9 };
const HIGH_BAND: BandRange = BandRange { start: 9, end: 16 };

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct BandLevels {
    pub(crate) bass: f32,
    pub(crate) mid: f32,
    pub(crate) treble: f32,
}

impl BandLevels {
    pub(crate) fn lerp(self, target: Self, fraction: f32) -> Self {
        let Self { bass, mid, treble } = self;
        Self {
            bass: lerp(bass, target.bass, fraction),
            mid: lerp(mid, target.mid, fraction),
            treble: lerp(treble, target.treble, fraction),
        }
    }
}

pub(crate) fn band_mean(spectrum: &Spectrum, range: BandRange) -> f32 {
    let slice = spectrum.get(range.start..range.end).unwrap_or(&[]);
    if slice.is_empty() {
        return 0.0;
    }
    slice.iter().sum::<f32>() / dimension_f32(slice.len())
}

pub(crate) fn band_levels(spectrum: &Spectrum) -> BandLevels {
    BandLevels {
        bass: band_mean(spectrum, LOW_BAND),
        mid: band_mean(spectrum, MID_BAND),
        treble: band_mean(spectrum, HIGH_BAND),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mirror {
    None,
    Horizontal,
    Kaleido4,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MilkdropPreset {
    pub(crate) base_zoom: f32,
    pub(crate) base_rotation: f32,
    pub(crate) mirror: Mirror,
}

const DRIFT: MilkdropPreset = MilkdropPreset {
    base_zoom: 1.12,
    base_rotation: 0.04,
    mirror: Mirror::None,
};

const MIRRORED: MilkdropPreset = MilkdropPreset {
    base_zoom: 0.94,
    base_rotation: -0.05,
    mirror: Mirror::Horizontal,
};

const KALEIDO: MilkdropPreset = MilkdropPreset {
    base_zoom: 1.18,
    base_rotation: 0.07,
    mirror: Mirror::Kaleido4,
};

pub(crate) fn preset_for_seed(seed: u64) -> MilkdropPreset {
    match seed % 3 {
        0 => DRIFT,
        1 => MIRRORED,
        _ => KALEIDO,
    }
}

pub(crate) const DECAY: f32 = 0.85;
pub(crate) const LEVEL_GLIDE: f32 = 0.15;
pub(crate) const ZOOM_GAIN: f32 = 0.1;
pub(crate) const ROTATION_GAIN: f32 = 0.5;
pub(crate) const CORE_RADIUS: f32 = 1.2;
pub(crate) const CORE_GAIN: f32 = 0.6;
pub(crate) const SPARK_COUNT: u32 = 3;
pub(crate) const ASPECT_X: f32 = 0.5;

pub(crate) const RAMP: [&str; 5] = [" ", "░", "▒", "▓", "█"];
pub(crate) const RAMP_FALLBACK: &str = "█";

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
    pub(crate) x: f32,
    pub(crate) y: f32,
}

pub(crate) fn field_center(size: FieldSize) -> FieldCenter {
    FieldCenter {
        x: dimension_f32(size.width.saturating_sub(1)) / 2.0,
        y: dimension_f32(size.height.saturating_sub(1)) / 2.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaneOffset {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

pub(crate) fn physical_offset(
    position: CellPosition,
    center: FieldCenter,
    aspect_x: f32,
) -> PlaneOffset {
    PlaneOffset {
        x: (dimension_f32(position.column) - center.x) * aspect_x,
        y: dimension_f32(position.row) - center.y,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Warp {
    pub(crate) center: FieldCenter,
    pub(crate) zoom: f32,
    pub(crate) sin: f32,
    pub(crate) cos: f32,
    pub(crate) aspect_x: f32,
}

pub(crate) fn warp_source(position: CellPosition, warp: &Warp) -> PlaneOffset {
    let offset = physical_offset(position, warp.center, warp.aspect_x);
    let scaled_offset = PlaneOffset {
        x: offset.x / warp.zoom,
        y: offset.y / warp.zoom,
    };
    let rotated_offset = PlaneOffset {
        x: scaled_offset.x * warp.cos - scaled_offset.y * warp.sin,
        y: scaled_offset.x * warp.sin + scaled_offset.y * warp.cos,
    };
    PlaneOffset {
        x: warp.center.x + rotated_offset.x / warp.aspect_x,
        y: warp.center.y + rotated_offset.y,
    }
}

pub(crate) fn bilinear_sample(
    cells: &[f32],
    size: FieldSize,
    plane_offset: PlaneOffset,
) -> f32 {
    let last_column = dimension_f32(size.width.saturating_sub(1));
    let last_row = dimension_f32(size.height.saturating_sub(1));
    let is_out_of_bounds = plane_offset.x < 0.0
        || plane_offset.y < 0.0
        || plane_offset.x > last_column
        || plane_offset.y > last_row;
    if is_out_of_bounds {
        return 0.0;
    }

    let column_low = floor::<usize>(plane_offset.x);
    let row_low = floor::<usize>(plane_offset.y);
    let column_high = (column_low + 1).min(size.width.saturating_sub(1));
    let row_high = (row_low + 1).min(size.height.saturating_sub(1));
    let column_fraction = plane_offset.x - dimension_f32(column_low);
    let row_fraction = plane_offset.y - dimension_f32(row_low);

    let sample = |row: usize, column: usize| {
        cells.get(row * size.width + column).copied().unwrap_or(0.0)
    };
    let top = lerp(
        sample(row_low, column_low),
        sample(row_low, column_high),
        column_fraction,
    );
    let bottom = lerp(
        sample(row_high, column_low),
        sample(row_high, column_high),
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
    let shifted_left = state ^ (state << 13);
    let shifted_right = shifted_left ^ (shifted_left >> 7);
    shifted_right ^ (shifted_right << 17)
}

pub(crate) fn inject(cells: &mut [f32], size: FieldSize, injection: &Injection) {
    for (row, cells_row) in row_chunks_mut(cells, size).enumerate() {
        for (column, cell) in cells_row.iter_mut().enumerate() {
            let position = CellPosition { column, row };
            let offset =
                physical_offset(position, injection.center, injection.aspect_x);
            let distance = (offset.x * offset.x + offset.y * offset.y).sqrt();
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
    for (mirrored_row, source_row) in
        row_chunks_mut(dest, size).zip(row_chunks(source, size))
    {
        write_mirrored_row(mirrored_row, source_row, size.width);
    }
}

pub(crate) fn write_mirrored_row(
    mirrored_row: &mut [f32],
    source_row: &[f32],
    width: usize,
) {
    for (column, cell) in mirrored_row.iter_mut().enumerate() {
        *cell = source_row
            .get(mirrored(column, width))
            .copied()
            .unwrap_or(0.0);
    }
}

pub(crate) fn mirrored(coordinate: usize, extent: usize) -> usize {
    if coordinate < extent / 2 {
        coordinate
    } else {
        extent.saturating_sub(1).saturating_sub(coordinate)
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
    for (row, mirrored_row) in row_chunks_mut(dest, size).enumerate() {
        let source_row = row_chunks(source, size)
            .nth(mirrored(row, size.height))
            .unwrap_or(&[]);
        write_mirrored_row(mirrored_row, source_row, size.width);
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::milkdrop::field::{
        BandLevels,
        CellPosition,
        FieldCenter,
        FieldSize,
        Injection,
        MilkdropPreset,
        Mirror,
        PlaneOffset,
        Warp,
        band_levels,
        bilinear_sample,
        inject,
        mirrored,
        preset_for_seed,
        warp_source,
        xorshift64,
    };

    fn lit_cells(cells: &[f32]) -> Vec<usize> {
        cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| **cell > 0.0)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn band_levels_average_the_bins_of_each_band() {
        let mut spectrum = [0.9; 16];
        spectrum[..3].fill(0.6);
        spectrum[3..9].fill(0.3);
        let BandLevels { bass, mid, treble } = band_levels(&spectrum);
        for (level, expected) in [(bass, 0.6), (mid, 0.3), (treble, 0.9)] {
            assert!((level - expected).abs() < 1e-5, "{level} != {expected}");
        }
    }

    #[rstest]
    #[case::drift(0, MilkdropPreset { base_zoom: 1.12, base_rotation: 0.04, mirror: Mirror::None })]
    #[case::mirrored_turns_the_other_way(1, MilkdropPreset { base_zoom: 0.94, base_rotation: -0.05, mirror: Mirror::Horizontal })]
    #[case::kaleido(2, MilkdropPreset { base_zoom: 1.18, base_rotation: 0.07, mirror: Mirror::Kaleido4 })]
    #[case::wraps_after_three(3, MilkdropPreset { base_zoom: 1.12, base_rotation: 0.04, mirror: Mirror::None })]
    fn each_seed_picks_its_preset(#[case] seed: u64, #[case] expected: MilkdropPreset) {
        assert_eq!(preset_for_seed(seed), expected);
    }

    #[rstest]
    #[case::no_zoom_no_turn(1.0, (0.0, 1.0), PlaneOffset { x: 5.0, y: 4.0 })]
    #[case::zoom_and_turn(2.0, (0.6, 0.8), PlaneOffset { x: 2.6, y: 3.1 })]
    fn warp_source_shrinks_by_the_zoom_and_turns_around_the_center(
        #[case] zoom: f32,
        #[case] turn: (f32, f32),
        #[case] expected: PlaneOffset,
    ) {
        let (sin, cos) = turn;
        let warp = Warp {
            center: FieldCenter { x: 3.0, y: 2.0 },
            zoom,
            sin,
            cos,
            aspect_x: 0.5,
        };
        let source = warp_source(CellPosition { column: 5, row: 4 }, &warp);
        assert!(
            (source.x - expected.x).abs() < 1e-5
                && (source.y - expected.y).abs() < 1e-5,
            "{source:?} != {expected:?}"
        );
    }

    #[rstest]
    #[case::left_edge(PlaneOffset { x: 0.0, y: 1.0 }, 4.0)]
    #[case::top_edge(PlaneOffset { x: 1.0, y: 0.0 }, 2.0)]
    #[case::right_edge(PlaneOffset { x: 2.0, y: 1.0 }, 6.0)]
    #[case::bottom_edge(PlaneOffset { x: 1.0, y: 2.0 }, 8.0)]
    #[case::between_four_cells(PlaneOffset { x: 0.5, y: 0.5 }, 3.0)]
    #[case::left_of_the_field(PlaneOffset { x: -0.5, y: 1.0 }, 0.0)]
    #[case::above_the_field(PlaneOffset { x: 1.0, y: -0.5 }, 0.0)]
    #[case::right_of_the_field(PlaneOffset { x: 2.5, y: 1.0 }, 0.0)]
    #[case::below_the_field(PlaneOffset { x: 1.0, y: 2.5 }, 0.0)]
    fn bilinear_sample_reads_up_to_the_edges_and_nothing_outside(
        #[case] plane_offset: PlaneOffset,
        #[case] expected: f32,
    ) {
        let cells = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let size = FieldSize {
            width: 3,
            height: 3,
        };
        assert_eq!(bilinear_sample(&cells, size, plane_offset), expected);
    }

    #[rstest]
    #[case::one(1, 1_082_269_761)]
    #[case::golden_ratio(0x9E37_79B9_7F4A_7C15, 15_860_402_102_123_842_989)]
    fn xorshift64_shifts_by_13_7_17(#[case] state: u64, #[case] expected: u64) {
        assert_eq!(xorshift64(state), expected);
    }

    #[test]
    fn inject_lights_the_cells_within_the_core_radius_of_the_center() {
        let size = FieldSize {
            width: 5,
            height: 5,
        };
        let mut cells = vec![0.0; 25];
        inject(
            &mut cells,
            size,
            &Injection {
                center: FieldCenter { x: 2.0, y: 2.0 },
                aspect_x: 0.5,
                core_radius: 1.2,
                treble: 0.0,
                spark_count: 0,
                seed: 0,
                tick: 0,
            },
        );
        assert_eq!(lit_cells(&cells), [6, 7, 8, 10, 11, 12, 13, 14, 16, 17, 18]);
    }

    #[test]
    fn inject_scatters_the_treble_sparks_over_seeded_cells() {
        let size = FieldSize {
            width: 5,
            height: 4,
        };
        let mut cells = vec![0.0; 20];
        inject(
            &mut cells,
            size,
            &Injection {
                center: FieldCenter { x: 2.0, y: 1.5 },
                aspect_x: 0.5,
                core_radius: 0.0,
                treble: 0.5,
                spark_count: 3,
                seed: 7,
                tick: 2,
            },
        );
        assert_eq!(lit_cells(&cells), [6, 13, 15]);
    }

    #[rstest]
    #[case::near_half_of_even(1, 4, 1)]
    #[case::middle_of_even(2, 4, 1)]
    #[case::far_end_of_even(3, 4, 0)]
    #[case::first_of_even(0, 4, 0)]
    #[case::middle_of_odd(2, 5, 2)]
    #[case::far_end_of_odd(4, 5, 0)]
    fn mirrored_folds_the_far_half_onto_the_near_half(
        #[case] coordinate: usize,
        #[case] extent: usize,
        #[case] expected: usize,
    ) {
        assert_eq!(mirrored(coordinate, extent), expected);
    }
}
