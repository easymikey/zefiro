use std::sync::Arc;

use config::FormatChips;
use kernel::domain::Track;
use ratatui::text::Line;

use crate::primitive::{chip::ChipColors, format_chips};

const CHIP_GAP: usize = 2;

pub(crate) struct FormatChipFit {
    pub(crate) line: Option<Line<'static>>,
    pub(crate) elapsed_budget: usize,
}

pub(crate) struct FormatChipContent<'a> {
    pub(crate) current: Option<&'a Arc<Track>>,
    pub(crate) visibility: FormatChips,
    pub(crate) colors: ChipColors,
}

pub(crate) struct ChipBudget {
    pub(crate) available_width: u16,
    pub(crate) elapsed_width: usize,
}

pub(crate) fn format_chip_fit(
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
    let chip_budget = usize::from(row_width)
        .saturating_sub(elapsed_width)
        .saturating_sub(CHIP_GAP);
    let time_chip_line = matches!(visibility, FormatChips::Shown)
        .then(|| {
            current.and_then(|track| {
                format_chips::fit_format_chips(
                    track.audio_format(),
                    colors,
                    chip_budget,
                )
            })
        })
        .flatten();
    let time_chip_width = time_chip_line
        .as_ref()
        .map_or(0, |line| crate::primitive::span::width(&line.spans));
    let elapsed_budget =
        usize::from(row_width).saturating_sub(if time_chip_width > 0 {
            time_chip_width + CHIP_GAP
        } else {
            0
        });
    FormatChipFit {
        line: time_chip_line,
        elapsed_budget,
    }
}
