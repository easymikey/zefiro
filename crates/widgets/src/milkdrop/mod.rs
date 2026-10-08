pub mod cover;
mod field;

use std::sync::Arc;

use kernel::cmd::Playback;
use ratatui::{style::Color, text::Line};

use crate::{
    milkdrop::field::{
        ASPECT_X,
        BandLevels,
        COLOR_BAND_HIGH,
        CORE_GAIN,
        CORE_RADIUS,
        CellPosition,
        DECAY,
        FieldSize,
        Injection,
        LEVEL_GLIDE,
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
    pixels::numeric::{dimension_f32, round},
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
    band_levels: BandLevels,
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
            band_levels: BandLevels::default(),
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
        self.band_levels = self
            .band_levels
            .lerp(band_levels(milkdrop_advance.spectrum), LEVEL_GLIDE);
        let levels = self.band_levels;
        let preset = preset_for_seed(milkdrop_advance.seed);

        let size = FieldSize {
            width: self.width,
            height: self.height,
        };
        let center = field_center(size);
        let zoom = preset.base_zoom + levels.bass * ZOOM_GAIN;
        let rotation = preset.base_rotation + levels.mid * ROTATION_GAIN;
        let (sin, cos) = (-rotation).sin_cos();
        let warp = Warp {
            center,
            zoom,
            sin,
            cos,
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
    accent: Color,
    foreground: Color,
}

impl MilkdropStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let colors = theme.colors();
        Self {
            accent: colors.accent,
            foreground: colors.foreground,
        }
    }

    fn color_for(self, intensity: f32) -> Color {
        if intensity < COLOR_BAND_HIGH {
            self.accent
        } else {
            self.foreground
        }
    }
}

fn ramp_glyph(intensity: f32) -> &'static str {
    let last_index = RAMP.len() - 1;
    let index = round::<usize>(intensity.clamp(0.0, 1.0) * dimension_f32(last_index));
    RAMP.get(index.min(last_index))
        .copied()
        .unwrap_or(RAMP_FALLBACK)
}

pub(crate) fn lines(
    field: &MilkdropField,
    style: MilkdropStyle,
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
    use rstest::rstest;

    use crate::{
        milkdrop::{
            ASPECT_X,
            COLOR_BAND_HIGH,
            CellPosition,
            DECAY,
            FieldSize,
            Injection,
            MilkdropAdvance,
            MilkdropField,
            MilkdropStyle,
            SPARK_COUNT,
            Warp,
            bilinear_sample,
            field_center,
            inject,
            lines,
            warp_source,
        },
        pixels::numeric::dimension_f32,
    };

    const SILENT_BANDS: [f32; 16] = [0.0; 16];

    #[test]
    fn lines_renders_exactly_height_rows_of_exactly_width_cells() {
        let field = MilkdropField::new(20, 8);
        let style = MilkdropStyle {
            accent: Color::Red,
            foreground: Color::White,
        };
        let rendered = lines(&field, style);
        assert_eq!(rendered.len(), 8);
        for line in rendered.iter() {
            assert_eq!(line.spans.len(), 20);
        }
    }

    #[rstest]
    #[case::mid_band(0.35, ("░", Color::Red))]
    #[case::bright_band_edge(COLOR_BAND_HIGH, ("▓", Color::White))]
    fn a_cell_paints_its_ramp_glyph_in_the_accent_below_the_bright_band(
        #[case] intensity: f32,
        #[case] expected: (&str, Color),
    ) {
        let (glyph, color) = expected;
        let mut field = MilkdropField::new(1, 1);
        field.cells = vec![intensity];
        let style = MilkdropStyle {
            accent: Color::Red,
            foreground: Color::White,
        };
        let rendered = lines(&field, style);
        let span = &rendered[0].spans[0];
        assert_eq!((span.content.as_ref(), span.style.fg), (glyph, Some(color)));
    }

    #[rstest]
    #[case::silent_paused(0.0, Playback::Paused, (1.12, 0.04, None))]
    #[case::loud_paused(1.0, Playback::Paused, (1.135, 0.115, None))]
    #[case::loud_playing(1.0, Playback::Playing, (1.135, 0.115, Some((1.29, 0.15))))]
    fn advance_warps_by_the_preset_zoom_and_turn_then_injects_a_bass_sized_core(
        #[case] loudness: f32,
        #[case] playback: Playback,
        #[case] expected: (f32, f32, Option<(f32, f32)>),
    ) {
        let (zoom, rotation, injected) = expected;
        let size = FieldSize {
            width: 9,
            height: 7,
        };
        let mut field = MilkdropField::new(size.width, size.height);
        field.cells = (0..size.width * size.height)
            .map(|index| dimension_f32(index) / dimension_f32(size.width * size.height))
            .collect();
        let before = field.cells.clone();
        field.advance(&MilkdropAdvance {
            spectrum: &[loudness; 16],
            playback,
            seed: 0,
            tick: 5,
        });

        let center = field_center(size);
        let (sin, cos) = (-rotation).sin_cos();
        let warp = Warp {
            center,
            zoom,
            sin,
            cos,
            aspect_x: ASPECT_X,
        };
        let mut expected_cells: Vec<f32> = (0..size.height)
            .flat_map(|row| {
                (0..size.width).map(move |column| CellPosition { column, row })
            })
            .map(|position| {
                bilinear_sample(&before, size, warp_source(position, &warp)) * DECAY
            })
            .collect();
        if let Some((core_radius, treble)) = injected {
            inject(
                &mut expected_cells,
                size,
                &Injection {
                    center,
                    aspect_x: ASPECT_X,
                    core_radius,
                    treble,
                    spark_count: SPARK_COUNT,
                    seed: 0,
                    tick: 5,
                },
            );
        }
        assert_eq!(field.cells.len(), expected_cells.len());
        for (index, (cell, expected_cell)) in
            field.cells.iter().zip(&expected_cells).enumerate()
        {
            assert!(
                (cell - expected_cell).abs() < 1e-4,
                "cell {index}: {cell} != {expected_cell}"
            );
        }
    }

    #[test]
    fn ambient_zoom_above_one_spreads_the_core_outward_over_a_few_steps() {
        let (width, height) = (9, 9);
        let mut field = MilkdropField::new(width, height);
        let probe_position = CellPosition {
            column: width / 2 + 4,
            row: height / 2,
        };

        field.advance(&MilkdropAdvance {
            spectrum: &SILENT_BANDS,
            playback: Playback::Playing,
            seed: 0,
            tick: 0,
        });
        let initial = field.cell(probe_position);
        assert_eq!(initial, 0.0, "probe must start outside the injected core");

        for tick in 1..3 {
            field.advance(&MilkdropAdvance {
                spectrum: &SILENT_BANDS,
                playback: Playback::Playing,
                seed: 0,
                tick,
            });
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
        stepped_field.advance(&MilkdropAdvance {
            spectrum: &loud,
            playback: Playback::Playing,
            seed: 2,
            tick: 9,
        });

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

    #[test]
    fn horizontal_preset_output_is_left_right_symmetric() {
        let loud = [1.0; 16];
        let mut stepped_field = MilkdropField::new(10, 8);
        stepped_field.advance(&MilkdropAdvance {
            spectrum: &loud,
            playback: Playback::Playing,
            seed: 1,
            tick: 9,
        });

        for row in 0..stepped_field.height {
            for column in 0..stepped_field.width {
                assert_eq!(
                    stepped_field.cell(CellPosition { column, row }),
                    stepped_field.cell(CellPosition {
                        column: stepped_field.width - 1 - column,
                        row
                    }),
                    "not left-right symmetric at ({column}, {row})"
                );
            }
        }
    }

    #[test]
    fn loud_bands_leave_the_top_and_bottom_rows_below_the_bright_band() {
        let loud = [1.0; 16];
        for seed in 0..3 {
            let mut field = MilkdropField::new(20, 8);
            for tick in 0..30 {
                field.advance(&MilkdropAdvance {
                    spectrum: &loud,
                    playback: Playback::Playing,
                    seed,
                    tick,
                });
            }
            for row in [0, field.height - 1] {
                let total: f32 = (0..field.width)
                    .map(|column| field.cell(CellPosition { column, row }))
                    .sum();
                let mean = total / dimension_f32(field.width);
                assert!(
                    mean < COLOR_BAND_HIGH,
                    "seed {seed}: row {row} averages {mean}"
                );
            }
        }
    }

    #[test]
    fn one_loud_step_after_silence_changes_no_cell_by_half() {
        let loud = [1.0; 16];
        for seed in 0..3 {
            let mut field = MilkdropField::new(20, 8);
            for tick in 0..30 {
                field.advance(&MilkdropAdvance {
                    spectrum: &SILENT_BANDS,
                    playback: Playback::Playing,
                    seed,
                    tick,
                });
            }
            let settled = field.clone();
            field.advance(&MilkdropAdvance {
                spectrum: &loud,
                playback: Playback::Playing,
                seed,
                tick: 30,
            });
            for (index, (before, after)) in
                settled.cells.iter().zip(&field.cells).enumerate()
            {
                assert!(
                    (after - before).abs() < 0.5,
                    "seed {seed}: cell {index} went from {before} to {after}"
                );
            }
        }
    }
}
