use std::{borrow::Cow, time::Duration};

use kernel::domain::format_time;
use num_traits::ToPrimitive;
use raster::floor_usize;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        chip::{self, ChipColors},
        glyphs::{CardGlyphs, ProgressLineGlyphs},
        span::{row, text},
    },
    theme::FillColors,
};

const FULL_RUN: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
const EMPTY_RUN: &str = "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";
const VOLUME_RUN: &str = "████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████";

#[derive(Debug, Clone, Copy)]
struct FillGlyphs {
    filled_glyph: &'static str,
    filled_run: &'static str,
    partial: Option<&'static str>,
    groove_glyph: &'static str,
    groove_run: &'static str,
}

#[derive(Debug)]
pub(crate) struct FillSpec {
    frac: f32,
    width: usize,
    glyphs: FillGlyphs,
}

impl FillSpec {
    #[must_use]
    pub(crate) fn progress(frac: f32, width: usize) -> Self {
        let glyphs = ProgressLineGlyphs::default();
        Self {
            frac,
            width,
            glyphs: FillGlyphs {
                filled_glyph: glyphs.full,
                filled_run: FULL_RUN,
                partial: Some(glyphs.partial),
                groove_glyph: glyphs.empty,
                groove_run: EMPTY_RUN,
            },
        }
    }

    #[must_use]
    pub(crate) fn volume(frac: f32, width: usize) -> Self {
        let glyphs = CardGlyphs::default();
        Self {
            frac,
            width,
            glyphs: FillGlyphs {
                filled_glyph: glyphs.volume_filled,
                filled_run: VOLUME_RUN,
                partial: None,
                groove_glyph: glyphs.volume_empty,
                groove_run: VOLUME_RUN,
            },
        }
    }
}

fn run(glyph: &'static str, full_run: &'static str, cells: usize) -> Cow<'static, str> {
    full_run
        .get(..cells.saturating_mul(glyph.len()))
        .map_or_else(|| Cow::Owned(glyph.repeat(cells)), Cow::Borrowed)
}

#[must_use]
pub(crate) fn fill_line(spec: &FillSpec, colors: FillColors) -> Line<'static> {
    let width_f32 = spec.width.to_f32().unwrap_or(f32::MAX);
    let exact = spec.frac.clamp(0.0, 1.0) * width_f32;
    let whole = floor_usize(exact);
    let whole_f32 = whole.to_f32().unwrap_or(f32::MAX);
    let rounds_up = exact - whole_f32 >= 0.5 && whole < spec.width;
    let used = whole + usize::from(rounds_up);
    let partial = rounds_up.then_some(spec.glyphs.partial).flatten();
    let solid = used.saturating_sub(usize::from(partial.is_some()));
    let filled = run(spec.glyphs.filled_glyph, spec.glyphs.filled_run, solid);
    let groove = run(
        spec.glyphs.groove_glyph,
        spec.glyphs.groove_run,
        spec.width.saturating_sub(used),
    );
    row([
        Some(text(filled).fg(colors.accent)),
        partial.map(|glyph| text(glyph).fg(colors.accent)),
        Some(text(groove).fg(colors.dim)),
    ]
    .into_iter()
    .flatten())
}

#[derive(Debug)]
struct TimecodeFormat {
    remaining_sign: char,
}

impl Default for TimecodeFormat {
    fn default() -> Self {
        Self {
            remaining_sign: '-',
        }
    }
}

#[must_use]
pub(crate) fn remaining_label(remaining: Duration) -> String {
    let format = TimecodeFormat::default();
    format!("{}{}", format.remaining_sign, format_time(remaining))
}

#[derive(Debug)]
pub(crate) struct HudProgressLayout {
    pub gap: &'static str,
}

impl Default for HudProgressLayout {
    fn default() -> Self {
        Self { gap: "  " }
    }
}

#[must_use]
pub(crate) fn remaining_reserve(row_width: u16, remaining: Option<Duration>) -> u16 {
    let Some(remaining) = remaining else {
        return 0;
    };
    let gap = HudProgressLayout::default().gap.width();
    let reserved = chip::width(&remaining_label(remaining))
        .saturating_add(u16::try_from(gap).unwrap_or(u16::MAX));
    if row_width <= reserved { 0 } else { reserved }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressRow {
    pub frac: f32,
    pub row_width: usize,
    pub remaining: Duration,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressColors {
    pub bar: FillColors,
    pub chip: ChipColors,
}

#[must_use]
pub(crate) fn hud_progress_bar_width(row_width: u16, remaining: Duration) -> u16 {
    row_width.saturating_sub(remaining_reserve(row_width, Some(remaining)))
}

#[must_use]
pub(crate) fn hud_progress_line(
    input: &HudProgressRow,
    colors: &HudProgressColors,
) -> Line<'static> {
    let chip_spans = chip::spans(&remaining_label(input.remaining), colors.chip);
    let gap = HudProgressLayout::default().gap;
    let row_width = u16::try_from(input.row_width).unwrap_or(u16::MAX);
    let bar_width = usize::from(hud_progress_bar_width(row_width, input.remaining));
    if bar_width == input.row_width {
        return fill_line(&FillSpec::progress(input.frac, input.row_width), colors.bar);
    }
    let bar = fill_line(&FillSpec::progress(input.frac, bar_width), colors.bar);
    Line::from_iter(
        bar.spans
            .into_iter()
            .chain([Span::raw(gap)])
            .chain(chip_spans),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{style::Color, symbols::block, text::Line};
    use rstest::rstest;

    use crate::{
        primitive::{
            bar::{
                FillSpec,
                HudProgressColors,
                HudProgressRow,
                fill_line,
                hud_progress_line,
            },
            chip::ChipColors,
            glyphs::{CardGlyphs, ProgressLineGlyphs},
        },
        theme::FillColors,
    };

    fn painted(spec: &FillSpec) -> Line<'static> {
        fill_line(
            spec,
            FillColors {
                accent: Color::Green,
                dim: Color::Black,
            },
        )
    }

    fn progress_text(frac: f32, width: usize) -> String {
        painted(&FillSpec::progress(frac, width))
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::empty(0.0, 10)]
    #[case::half(0.5, 10)]
    #[case::just_short_of_full(0.95, 10)]
    #[case::full(1.0, 10)]
    fn the_progress_line_is_box_drawing_with_at_most_one_partial_cell(
        #[case] frac: f32,
        #[case] width: usize,
    ) {
        let text = progress_text(frac, width);
        for banned in [
            block::ONE_EIGHTH,
            block::ONE_QUARTER,
            block::THREE_EIGHTHS,
            block::HALF,
            block::FIVE_EIGHTHS,
            block::THREE_QUARTERS,
            block::SEVEN_EIGHTHS,
            block::FULL,
        ] {
            assert!(
                !text.contains(banned),
                "emitted banned block glyph {banned:?}"
            );
        }
        assert!(
            text.matches(ProgressLineGlyphs::default().partial).count() <= 1,
            "at most one partial cell"
        );
        assert_eq!(text.chars().count(), width);
        insta::with_settings!({ snapshot_suffix => format!("{frac}") }, {
            insta::assert_snapshot!(text);
        });
    }

    fn hud_progress_text(frac: f32, row_width: usize, remaining: Duration) -> String {
        let line = hud_progress_line(
            &HudProgressRow {
                frac,
                row_width,
                remaining,
            },
            &HudProgressColors {
                bar: FillColors {
                    accent: Color::Red,
                    dim: Color::Black,
                },
                chip: ChipColors {
                    border: Color::Black,
                    value: Color::White,
                },
            },
        );
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    struct BarRow {
        name: &'static str,
        frac: f32,
        row_width: usize,
        remaining_secs: u64,
    }

    #[rstest]
    #[case::at_the_start(BarRow { name: "start", frac: 0.0, row_width: 30, remaining_secs: 105 })]
    #[case::half_way(BarRow { name: "half", frac: 0.5, row_width: 30, remaining_secs: 30 })]
    #[case::at_the_end(BarRow { name: "end", frac: 1.0, row_width: 30, remaining_secs: 0 })]
    #[case::too_narrow_for_a_chip(BarRow { name: "narrow", frac: 0.5, row_width: 4, remaining_secs: 45 })]
    fn the_hud_row_paints_a_bar_and_its_remaining_chip(#[case] row: BarRow) {
        let text = hud_progress_text(
            row.frac,
            row.row_width,
            Duration::from_secs(row.remaining_secs),
        );
        assert_eq!(
            text.chars().count(),
            row.row_width,
            "the row fills its width"
        );
        insta::with_settings!({ snapshot_suffix => row.name }, {
            insta::assert_snapshot!(text);
        });
    }

    fn volume_text(frac: f32, bar_width: usize) -> String {
        painted(&FillSpec::volume(frac, bar_width))
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::silent(0.0)]
    #[case::half(0.5)]
    #[case::full(1.0)]
    fn the_volume_bar_is_solid_blocks_filling_every_row(#[case] frac: f32) {
        let glyphs = CardGlyphs::default();
        let row = volume_text(frac, 16);

        assert_eq!(row.chars().count(), 16, "the row fills the bar's width");
        assert_eq!(
            row.matches(ProgressLineGlyphs::default().full).count(),
            0,
            "the volume bar must not borrow the progress line's glyphs"
        );
        assert_ne!(row, progress_text(frac, 16));
        assert!(
            row.contains(glyphs.volume_filled) || row.contains(glyphs.volume_empty)
        );
        insta::with_settings!({ snapshot_suffix => format!("{frac}") }, {
            insta::assert_snapshot!(row);
        });
    }

    #[rstest]
    #[case::silent(0.0, 0)]
    #[case::half(0.5, 8)]
    #[case::full(1.0, 16)]
    fn the_volume_run_is_one_solid_block_that_changes_colour_at_the_level(
        #[case] frac: f32,
        #[case] filled: usize,
    ) {
        let glyphs = CardGlyphs::default();
        let line = painted(&FillSpec::volume(frac, 16));
        let cells: Vec<(char, Option<Color>)> = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(|glyph| (glyph, span.style.fg)))
            .collect();
        assert_eq!(cells.len(), 16, "the run covers the whole rect");
        assert_eq!(
            glyphs.volume_filled, glyphs.volume_empty,
            "both halves are one glyph"
        );
        let solid = glyphs.volume_filled.chars().next();
        assert!(
            cells.iter().all(|&(glyph, _)| Some(glyph) == solid),
            "every cell of the run is that solid block"
        );
        assert_eq!(
            cells
                .iter()
                .filter(|&&(_, fg)| fg == Some(Color::Green))
                .count(),
            filled,
            "the fill colour reaches exactly the level"
        );
    }
}
