use std::{borrow::Cow, time::Duration};

use kernel::domain::format_time;
use num_traits::ToPrimitive;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::{
    pixels::floor,
    primitive::{
        chip::{self, ChipStyle},
        glyphs,
        span::{line, text},
    },
    theme::{ActiveTheme, BarStyle},
};

const FULL_RUN: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
const EMPTY_RUN: &str = "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";
const VOLUME_RUN: &str = "████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████";

#[derive(Debug)]
pub(crate) struct BarFill {
    fraction: f32,
    width: usize,
    filled_glyph: &'static str,
    filled_run: &'static str,
    partial: Option<&'static str>,
    groove_glyph: &'static str,
    groove_run: &'static str,
}

impl BarFill {
    #[must_use]
    pub(crate) fn progress(fraction: f32, width: usize) -> Self {
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
    pub(crate) fn volume(fraction: f32, width: usize) -> Self {
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

#[must_use]
pub(crate) fn fill(spec: &BarFill, style: BarStyle) -> Line<'static> {
    let width_f32 = spec.width.to_f32().unwrap_or(f32::MAX);
    let exact = spec.fraction.clamp(0.0, 1.0) * width_f32;
    let whole = floor::<usize>(exact);
    let whole_f32 = whole.to_f32().unwrap_or(f32::MAX);
    let rounds_up = exact - whole_f32 >= 0.5 && whole < spec.width;
    let used = whole + usize::from(rounds_up);
    let partial = rounds_up.then_some(spec.partial).flatten();
    let solid = used.saturating_sub(usize::from(partial.is_some()));
    let filled = repeat_glyph(spec.filled_glyph, spec.filled_run, solid);
    let groove = repeat_glyph(
        spec.groove_glyph,
        spec.groove_run,
        spec.width.saturating_sub(used),
    );
    line(
        [
            Some(text(filled).fg(style.fill)),
            partial.map(|glyph| text(glyph).fg(style.fill)),
            Some(text(groove).fg(style.track)),
        ]
        .into_iter()
        .flatten(),
    )
}

const REMAINING_SIGN: char = '-';
const HUD_GAP: &str = "  ";

#[must_use]
pub(crate) fn remaining_label(remaining: Duration) -> String {
    format!("{REMAINING_SIGN}{}", format_time(remaining))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressRow {
    pub fraction: f32,
    pub row_width: usize,
    pub remaining: Duration,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressStyle {
    pub bar: BarStyle,
    pub chip: ChipStyle,
}

impl HudProgressStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            bar: BarStyle::progress(theme),
            chip: ChipStyle::from_theme(theme),
        }
    }
}

#[must_use]
pub(crate) fn hud_progress_bar_width(row_width: u16, remaining: Duration) -> u16 {
    let gap = u16::try_from(HUD_GAP.width()).unwrap_or(u16::MAX);
    let reserved = chip::width(&remaining_label(remaining)).saturating_add(gap);
    if row_width <= reserved {
        row_width
    } else {
        row_width - reserved
    }
}

#[must_use]
pub(crate) fn hud_progress_line(
    input: &HudProgressRow,
    colors: &HudProgressStyle,
) -> Line<'static> {
    let chip_spans = chip::spans(&remaining_label(input.remaining), colors.chip);
    let row_width = u16::try_from(input.row_width).unwrap_or(u16::MAX);
    let bar_width = usize::from(hud_progress_bar_width(row_width, input.remaining));
    if bar_width == input.row_width {
        return fill(
            &BarFill::progress(input.fraction, input.row_width),
            colors.bar,
        );
    }
    let bar = fill(&BarFill::progress(input.fraction, bar_width), colors.bar);
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

    use ratatui::{style::Color, symbols::block, text::Line};
    use rstest::rstest;

    use crate::{
        primitive::{
            bar::{BarFill, HudProgressRow, HudProgressStyle, fill, hud_progress_line},
            chip::ChipStyle,
            glyphs,
        },
        theme::BarStyle,
    };

    fn painted(spec: &BarFill) -> Line<'static> {
        fill(
            spec,
            BarStyle {
                fill: Color::Green,
                track: Color::Black,
            },
        )
    }

    fn progress_text(fraction: f32, width: usize) -> String {
        painted(&BarFill::progress(fraction, width))
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
        #[case] width: usize,
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
        assert_eq!(text.chars().count(), width);
        insta::with_settings!({ snapshot_suffix => format!("{fraction}") }, {
            insta::assert_snapshot!(text);
        });
    }

    fn hud_progress_text(
        fraction: f32,
        row_width: usize,
        remaining: Duration,
    ) -> String {
        let line = hud_progress_line(
            &HudProgressRow {
                fraction,
                row_width,
                remaining,
            },
            &HudProgressStyle {
                bar: BarStyle {
                    fill: Color::Red,
                    track: Color::Black,
                },
                chip: ChipStyle {
                    border: Color::Black,
                    foreground: Color::White,
                },
            },
        );
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    struct BarRow {
        name: &'static str,
        fraction: f32,
        row_width: usize,
        remaining_secs: u64,
    }

    #[rstest]
    #[case::at_the_start(BarRow { name: "start", fraction: 0.0, row_width: 30, remaining_secs: 105 })]
    #[case::half_way(BarRow { name: "half", fraction: 0.5, row_width: 30, remaining_secs: 30 })]
    #[case::at_the_end(BarRow { name: "end", fraction: 1.0, row_width: 30, remaining_secs: 0 })]
    #[case::too_narrow_for_a_chip(BarRow { name: "narrow", fraction: 0.5, row_width: 4, remaining_secs: 45 })]
    fn the_hud_row_paints_a_bar_and_its_remaining_chip(#[case] row: BarRow) {
        let text = hud_progress_text(
            row.fraction,
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

    fn volume_text(fraction: f32, bar_width: usize) -> String {
        painted(&BarFill::volume(fraction, bar_width))
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
        assert!(
            row.contains(glyphs::VOLUME_BLOCK) || row.contains(glyphs::VOLUME_BLOCK)
        );
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
        let line = painted(&BarFill::volume(fraction, 16));
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
