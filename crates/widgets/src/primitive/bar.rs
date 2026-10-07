use std::{borrow::Cow, time::Duration};

use kernel::domain::geometry::Cells;
use ratatui::{
    style::Color,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    pixels::numeric::{dimension_f32, floor, small_count_u16},
    primitive::{
        chip,
        glyphs,
        span::{line, text},
        time_text::duration_text,
    },
    theme::colors::Colors,
};

const FULL_RUN: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
const EMPTY_RUN: &str = "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";
const VOLUME_RUN: &str = "████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████";

#[derive(Debug)]
pub(crate) struct BarFill {
    fraction: f32,
    width: Cells,
    filled_glyph: &'static str,
    filled_run: &'static str,
    partial: Option<&'static str>,
    groove_glyph: &'static str,
    groove_run: &'static str,
}

impl BarFill {
    #[must_use]
    pub(crate) fn progress(fraction: f32, width: Cells) -> Self {
        Self {
            fraction,
            width,
            filled_glyph: glyphs::progress_line::FULL,
            filled_run: FULL_RUN,
            partial: Some(glyphs::progress_line::PARTIAL),
            groove_glyph: glyphs::progress_line::EMPTY,
            groove_run: EMPTY_RUN,
        }
    }

    #[must_use]
    pub(crate) fn volume(fraction: f32, width: Cells) -> Self {
        Self {
            fraction,
            width,
            filled_glyph: glyphs::VOLUME_BLOCK,
            filled_run: VOLUME_RUN,
            partial: None,
            groove_glyph: glyphs::VOLUME_BLOCK,
            groove_run: VOLUME_RUN,
        }
    }

    #[must_use]
    pub(crate) fn line(&self, fill_color: Color, groove: Color) -> Line<'static> {
        let width = self.width.count();
        let width_f32 = dimension_f32(width);
        let exact = self.fraction.clamp(0.0, 1.0) * width_f32;
        let whole = floor::<usize>(exact);
        let whole_f32 = dimension_f32(whole);
        let rounds_up = exact - whole_f32 >= 0.5 && whole < width;
        let used = whole + usize::from(rounds_up);
        let partial = rounds_up.then_some(self.partial).flatten();
        let solid = used.saturating_sub(usize::from(partial.is_some()));
        let filled = repeat_glyph(self.filled_glyph, self.filled_run, solid);
        let empty = repeat_glyph(
            self.groove_glyph,
            self.groove_run,
            width.saturating_sub(used),
        );
        line(
            [
                Some(text(filled).fg(fill_color)),
                partial.map(|glyph| text(glyph).fg(fill_color)),
                Some(text(empty).fg(groove)),
            ]
            .into_iter()
            .flatten(),
        )
    }
}

fn repeat_glyph(
    glyph: &'static str,
    full_run: &'static str,
    cells: usize,
) -> Cow<'static, str> {
    full_run
        .get(..cells.saturating_mul(glyph.len()))
        .map_or_else(|| Cow::Owned(glyph.repeat(cells)), Cow::Borrowed)
}

const REMAINING_SIGN: char = '-';
const HUD_GAP: &str = "  ";

#[must_use]
pub(crate) fn remaining_label(remaining: Duration) -> String {
    format!("{REMAINING_SIGN}{}", duration_text(remaining))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgress<'a> {
    pub(crate) fraction: f32,
    pub(crate) row_width: Cells,
    pub(crate) bar_width: Cells,
    pub(crate) remaining_label: &'a str,
    pub(crate) fill: Color,
    pub(crate) groove: Color,
}

fn reserved_cells(label: &str) -> u16 {
    chip::width(label)
        .0
        .saturating_add(small_count_u16(HUD_GAP.width()))
}

#[must_use]
pub(crate) fn hud_progress_bar_width(row_width: Cells, remaining_label: &str) -> Cells {
    let reserved = reserved_cells(remaining_label);
    if row_width.0 <= reserved {
        row_width
    } else {
        Cells(row_width.0 - reserved)
    }
}

#[must_use]
pub(crate) fn hud_progress_line<'a>(
    hud_progress: &HudProgress<'a>,
    colors: &Colors<Color>,
) -> Line<'a> {
    let bar = BarFill::progress(hud_progress.fraction, hud_progress.bar_width)
        .line(hud_progress.fill, hud_progress.groove);
    if hud_progress.bar_width == hud_progress.row_width {
        return bar;
    }
    let chip_spans = chip::spans(hud_progress.remaining_label, colors);
    Line::from_iter(
        bar.spans
            .into_iter()
            .chain([Span::raw(HUD_GAP)])
            .chain(chip_spans),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::geometry::Cells;
    use ratatui::{style::Color, symbols::block, text::Line};
    use rstest::rstest;

    use crate::{
        primitive::{
            bar::{
                BarFill,
                HudProgress,
                hud_progress_bar_width,
                hud_progress_line,
                remaining_label,
            },
            glyphs,
        },
        theme::colors::Colors,
    };

    fn painted(bar_fill: &BarFill) -> Line<'static> {
        bar_fill.line(Color::Green, Color::Black)
    }

    fn progress_text(fraction: f32, width: u16) -> String {
        painted(&BarFill::progress(fraction, Cells(width)))
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
        #[case] fraction: f32,
        #[case] width: u16,
    ) {
        let text = progress_text(fraction, width);
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
            text.matches(glyphs::progress_line::PARTIAL).count() <= 1,
            "at most one partial cell"
        );
        assert_eq!(text.chars().count(), usize::from(width));
        insta::with_settings!({ snapshot_suffix => format!("{fraction}") }, {
            insta::assert_snapshot!(text);
        });
    }

    fn hud_progress_text(fraction: f32, row_width: u16, remaining: Duration) -> String {
        let label = remaining_label(remaining);
        let line = hud_progress_line(
            &HudProgress {
                fraction,
                row_width: Cells(row_width),
                bar_width: hud_progress_bar_width(Cells(row_width), &label),
                remaining_label: &label,
                fill: Color::Red,
                groove: Color::Black,
            },
            &Colors {
                muted_foreground: Color::Black,
                foreground: Color::White,
                ..Colors::default()
            },
        );
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    struct BarRow {
        name: &'static str,
        fraction: f32,
        row_width: u16,
        remaining_secs: u64,
    }

    #[rstest]
    #[case::at_the_start(BarRow { name: "start", fraction: 0.0, row_width: 30, remaining_secs: 105 })]
    #[case::half_way(BarRow { name: "half", fraction: 0.5, row_width: 30, remaining_secs: 30 })]
    #[case::at_the_end(BarRow { name: "end", fraction: 1.0, row_width: 30, remaining_secs: 0 })]
    #[case::too_narrow_for_a_chip(BarRow { name: "narrow", fraction: 0.5, row_width: 4, remaining_secs: 45 })]
    fn the_hud_progress_paints_a_bar_and_its_remaining_chip(#[case] row: BarRow) {
        let text = hud_progress_text(
            row.fraction,
            row.row_width,
            Duration::from_secs(row.remaining_secs),
        );
        assert_eq!(
            text.chars().count(),
            usize::from(row.row_width),
            "the row fills its width"
        );
        insta::with_settings!({ snapshot_suffix => row.name }, {
            insta::assert_snapshot!(text);
        });
    }

    fn volume_text(fraction: f32, bar_width: u16) -> String {
        painted(&BarFill::volume(fraction, Cells(bar_width)))
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::silent(0.0)]
    #[case::half(0.5)]
    #[case::full(1.0)]
    fn the_volume_bar_is_solid_blocks_filling_every_row(#[case] fraction: f32) {
        let row = volume_text(fraction, 16);

        assert_eq!(row.chars().count(), 16, "the row fills the bar's width");
        assert_eq!(
            row.matches(glyphs::progress_line::FULL).count(),
            0,
            "the volume bar must not borrow the progress line's glyphs"
        );
        assert_ne!(row, progress_text(fraction, 16));
        assert!(row.contains(glyphs::VOLUME_BLOCK));
        insta::with_settings!({ snapshot_suffix => format!("{fraction}") }, {
            insta::assert_snapshot!(row);
        });
    }

    #[rstest]
    #[case::silent(0.0, 0)]
    #[case::half(0.5, 8)]
    #[case::full(1.0, 16)]
    fn the_volume_run_is_one_solid_block_that_changes_colour_at_the_level(
        #[case] fraction: f32,
        #[case] filled: usize,
    ) {
        let line = painted(&BarFill::volume(fraction, Cells(16)));
        let cells: Vec<(char, Option<Color>)> = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(|glyph| (glyph, span.style.fg)))
            .collect();
        assert_eq!(cells.len(), 16, "the run covers the whole rect");
        let solid = glyphs::VOLUME_BLOCK.chars().next();
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
