pub mod cover;
mod field;

use std::sync::Arc;

use kernel::cmd::Playback;
use ratatui::{style::Color, text::Line};

use crate::{
    milkdrop::field::{
        ASPECT_X,
        COLOR_BAND_HIGH,
        COLOR_BAND_MID,
        CORE_GAIN,
        CORE_RADIUS,
        CellPosition,
        DECAY,
        FieldSize,
        Injection,
        Mirror,
        RAMP,
        RAMP_FALLBACK,
        ROTATION_GAIN,
        SPARK_COUNT,
        Warp,
        ZOOM_GAIN,
        band_levels,
        bilinear_sample,
        field_center,
        inject,
        kaleidoscope_quadrants_into,
        mirror_horizontal_into,
        preset_for_seed,
        warp_source,
    },
    primitive::span::text,
    spectrum::Spectrum,
    theme::active_theme::ActiveTheme,
};

fn resolve_mirror(field: &mut MilkdropField, size: FieldSize, mirror: Mirror) {
    match mirror {
        Mirror::None => std::mem::swap(&mut field.cells, &mut field.scratch),
        Mirror::Horizontal => {
            mirror_horizontal_into(&field.scratch, &mut field.cells, size);
        }
        Mirror::Kaleido4 => {
            kaleidoscope_quadrants_into(&field.scratch, &mut field.cells, size);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MilkdropField {
    cells: Vec<f32>,
    scratch: Vec<f32>,
    width: usize,
    height: usize,
}

impl MilkdropField {
    #[must_use]
    pub(crate) fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            cells: vec![0.0; width * height],
            scratch: vec![0.0; width * height],
            width,
            height,
        }
    }

    fn cell(&self, position: CellPosition) -> f32 {
        self.cells
            .get(position.row * self.width + position.column)
            .copied()
            .unwrap_or(0.0)
    }
}

#[derive(Debug)]
pub(crate) struct MilkdropAdvance<'a> {
    pub(crate) spectrum: &'a Spectrum,
    pub(crate) playback: Playback,
    pub(crate) seed: u64,
    pub(crate) tick: u64,
}

impl MilkdropField {
    pub(crate) fn advance(&mut self, milkdrop_advance: &MilkdropAdvance<'_>) {
        let levels = band_levels(milkdrop_advance.spectrum);
        let preset = preset_for_seed(milkdrop_advance.seed);

        let size = FieldSize {
            width: self.width,
            height: self.height,
        };
        let center = field_center(size);
        let zoom = preset.base_zoom + levels.bass * ZOOM_GAIN;
        let rotation = preset.base_rotation + levels.mid * ROTATION_GAIN;
        let warp = Warp {
            center,
            zoom,
            rotation,
            aspect_x: ASPECT_X,
        };

        self.scratch.clear();
        for row in 0..self.height {
            for column in 0..self.width {
                let source = warp_source(CellPosition { column, row }, &warp);
                let warped = bilinear_sample(&self.cells, size, source);
                self.scratch.push(warped * DECAY);
            }
        }

        if milkdrop_advance.playback == Playback::Playing {
            inject(
                &mut self.scratch,
                size,
                &Injection {
                    center,
                    aspect_x: ASPECT_X,
                    core_radius: CORE_RADIUS + levels.bass * CORE_GAIN,
                    treble: levels.treble,
                    spark_count: SPARK_COUNT,
                    seed: milkdrop_advance.seed,
                    tick: milkdrop_advance.tick,
                },
            );
        }

        resolve_mirror(self, size, preset.mirror);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MilkdropStyle {
    muted_foreground: Color,
    accent: Color,
    foreground: Color,
}

impl MilkdropStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let colors = theme.colors();
        Self {
            muted_foreground: colors.muted_foreground,
            accent: colors.accent,
            foreground: colors.foreground,
        }
    }

    fn color_for(&self, intensity: f32) -> Color {
        if intensity < COLOR_BAND_MID {
            self.muted_foreground
        } else if intensity < COLOR_BAND_HIGH {
            self.accent
        } else {
            self.foreground
        }
    }
}

fn ramp_glyph(intensity: f32) -> &'static str {
    let last_index = RAMP.len() - 1;
    let clamped = intensity.clamp(0.0, 1.0);
    let index = crate::pixels::numeric::round::<usize>(
        clamped * crate::pixels::numeric::dimension_f32(last_index),
    );
    RAMP.get(index.min(last_index))
        .copied()
        .unwrap_or(RAMP_FALLBACK)
}

pub(crate) fn lines(
    field: &MilkdropField,
    style: &MilkdropStyle,
) -> Arc<[Line<'static>]> {
    (0..field.height)
        .map(|row| {
            (0..field.width)
                .map(|column| {
                    let intensity = field.cell(CellPosition { column, row });
                    text(ramp_glyph(intensity)).fg(style.color_for(intensity))
                })
                .collect::<Line<'static>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use kernel::cmd::Playback;
    use ratatui::style::Color;

    use crate::{
        milkdrop::{
            CellPosition,
            MilkdropAdvance,
            MilkdropField,
            MilkdropStyle,
            lines,
        },
        spectrum::Spectrum,
    };

    const SILENT_BANDS: [f32; 16] = [0.0; 16];

    fn input(
        spectrum: &Spectrum,
        playback: Playback,
        beat: (u64, u64),
    ) -> MilkdropAdvance<'_> {
        MilkdropAdvance {
            spectrum,
            playback,
            seed: beat.0,
            tick: beat.1,
        }
    }

    #[test]
    fn lines_renders_exactly_height_rows_of_exactly_width_cells() {
        let field = MilkdropField::new(20, 8);
        let style = MilkdropStyle {
            muted_foreground: Color::Black,
            accent: Color::Red,
            foreground: Color::White,
        };
        let rendered = lines(&field, &style);
        assert_eq!(rendered.len(), 8);
        for line in rendered.iter() {
            assert_eq!(line.spans.len(), 20);
        }
    }

    #[test]
    fn lines_matches_the_seven_row_variant_too() {
        let field = MilkdropField::new(20, 7);
        let style = MilkdropStyle {
            muted_foreground: Color::Black,
            accent: Color::Red,
            foreground: Color::White,
        };
        assert_eq!(lines(&field, &style).len(), 7);
    }

    #[test]
    fn a_degenerate_zero_size_request_still_produces_a_one_by_one_field() {
        let field = MilkdropField::new(0, 0);
        assert_eq!((field.width, field.height), (1, 1));
    }

    #[test]
    fn step_is_deterministic_for_the_same_seed_and_tick() {
        let mut first_field = MilkdropField::new(9, 9);
        let mut second_field = first_field.clone();
        first_field.advance(&input(&SILENT_BANDS, Playback::Playing, (7, 3)));
        second_field.advance(&input(&SILENT_BANDS, Playback::Playing, (7, 3)));
        assert_eq!(first_field, second_field);
    }

    #[test]
    fn different_seeds_produce_different_fields() {
        let loud_treble = [1.0; 16];
        let mut first_field = MilkdropField::new(9, 9);
        let mut second_field = first_field.clone();
        first_field.advance(&input(&loud_treble, Playback::Playing, (0, 5)));
        second_field.advance(&input(&loud_treble, Playback::Playing, (1, 5)));
        assert_ne!(first_field, second_field);
    }

    #[test]
    fn same_seed_scatters_sparks_at_the_same_positions_across_independent_fields() {
        let loud_treble = [1.0; 16];
        let mut first_field = MilkdropField::new(9, 9);
        let mut second_field = MilkdropField::new(9, 9);
        first_field.advance(&input(&loud_treble, Playback::Playing, (42, 11)));
        second_field.advance(&input(&loud_treble, Playback::Playing, (42, 11)));
        assert_eq!(first_field, second_field);
    }

    #[test]
    fn decay_only_steps_converge_to_near_zero() {
        let mut field = MilkdropField::new(9, 9);
        field.cells = vec![1.0; field.cells.len()];
        for tick in 0..80 {
            field.advance(&input(&SILENT_BANDS, Playback::Paused, (3, tick)));
        }
        assert!(
            field.cells.iter().all(|&level| level < 0.001),
            "expected a fully decayed field, got {:?}",
            field.cells
        );
    }

    #[test]
    fn ambient_zoom_above_one_spreads_the_core_outward_over_a_few_steps() {
        let (width, height) = (9, 9);
        let mut field = MilkdropField::new(width, height);
        let probe_position = CellPosition {
            column: width / 2 + 4,
            row: height / 2,
        };

        field.advance(&input(&SILENT_BANDS, Playback::Playing, (0, 0)));
        let initial = field.cell(probe_position);
        assert_eq!(initial, 0.0, "probe must start outside the injected core");

        for tick in 1..3 {
            field.advance(&input(&SILENT_BANDS, Playback::Playing, (0, tick)));
        }
        let spread = field.cell(probe_position);
        assert!(
            spread > initial,
            "expected the core to have spread out to the probe cell by the third step, got {spread}"
        );
    }

    #[test]
    fn kaleido_preset_output_is_four_way_symmetric() {
        let loud = [1.0; 16];
        let mut stepped_field = MilkdropField::new(10, 8);
        stepped_field.advance(&input(&loud, Playback::Playing, (2, 9)));

        for row in 0..stepped_field.height {
            for column in 0..stepped_field.width {
                let cell = stepped_field.cell(CellPosition { column, row });
                let mirrored_column = stepped_field.width - 1 - column;
                let mirrored_row = stepped_field.height - 1 - row;
                assert_eq!(
                    cell,
                    stepped_field.cell(CellPosition {
                        column: mirrored_column,
                        row
                    }),
                    "not left-right symmetric at ({column}, {row})"
                );
                assert_eq!(
                    cell,
                    stepped_field.cell(CellPosition {
                        column,
                        row: mirrored_row
                    }),
                    "not top-bottom symmetric at ({column}, {row})"
                );
            }
        }
    }
}
