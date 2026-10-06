use std::sync::Arc;

use kernel::domain::{appearance::FormatChips, geometry::Cells, track::Track};
use ratatui::{style::Color, text::Line};

use crate::{primitive::format_chips, theme::colors::Colors};

const CHIP_GAP: usize = 2;

pub(crate) struct FormatChipFit {
    pub(crate) line: Option<Line<'static>>,
    pub(crate) elapsed_budget: usize,
}

pub(crate) struct FormatChipsInput<'a> {
    pub(crate) displayed_track: Option<&'a Arc<Track>>,
    pub(crate) format_chips: FormatChips,
    pub(crate) colors: &'a Colors<Color>,
}

pub(crate) struct ChipBudget {
    pub(crate) available_width: Cells,
    pub(crate) elapsed_width: usize,
}

impl FormatChipFit {
    pub(crate) fn new(
        input: &FormatChipsInput<'_>,
        budget: &ChipBudget,
    ) -> FormatChipFit {
        let &FormatChipsInput {
            displayed_track,
            format_chips,
            colors,
        } = input;
        let &ChipBudget {
            available_width: row_width,
            elapsed_width,
        } = budget;
        let chip_budget = row_width
            .count()
            .saturating_sub(elapsed_width)
            .saturating_sub(CHIP_GAP);
        let time_chip_line = matches!(format_chips, FormatChips::Shown)
            .then(|| {
                displayed_track.and_then(|track| {
                    format_chips::format_chips_line(
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
        let elapsed_budget = row_width.count().saturating_sub(if time_chip_width > 0 {
            time_chip_width + CHIP_GAP
        } else {
            0
        });
        FormatChipFit {
            line: time_chip_line,
            elapsed_budget,
        }
    }
}
