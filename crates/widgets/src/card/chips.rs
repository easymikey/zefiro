use std::sync::Arc;

use config::FormatChips;
use kernel::domain::Track;
use ratatui::text::Line;

use crate::primitive::{chip::ChipColors, format_chips};

#[derive(Debug, Clone, Copy, PartialEq)]
struct FormatChipsLayout {
    gap: usize,
}

impl Default for FormatChipsLayout {
    fn default() -> Self {
        Self { gap: 2 }
    }
}

pub(super) struct FormatChipFit {
    pub(super) line: Option<Line<'static>>,
    pub(super) elapsed_budget: usize,
}

pub(super) struct FormatChipContent<'a> {
    pub(super) current: Option<&'a Arc<Track>>,
    pub(super) visibility: FormatChips,
    pub(super) colors: ChipColors,
}

pub(super) struct ChipBudget {
    pub(super) available_width: u16,
    pub(super) elapsed_width: usize,
}

pub(super) fn format_chip_fit(
    input: &FormatChipContent<'_>,
    budget: &ChipBudget,
) -> FormatChipFit {
    let &FormatChipContent {
        current,
        visibility,
        colors,
    } = input;
    let &ChipBudget {
        available_width: row_width,
        elapsed_width,
    } = budget;
    let chip_gap = FormatChipsLayout::default().gap;
    let chip_budget = usize::from(row_width)
        .saturating_sub(elapsed_width)
        .saturating_sub(chip_gap);
    let time_chip_line = matches!(visibility, FormatChips::Shown)
        .then(|| {
            current.and_then(|track| {
                format_chips::build_fit(track.audio_format(), colors, chip_budget)
            })
        })
        .flatten();
    let time_chip_width = time_chip_line
        .as_ref()
        .map_or(0, |line| crate::primitive::span::width(&line.spans));
    let elapsed_budget =
        usize::from(row_width).saturating_sub(if time_chip_width > 0 {
            time_chip_width + chip_gap
        } else {
            0
        });
    FormatChipFit {
        line: time_chip_line,
        elapsed_budget,
    }
}
