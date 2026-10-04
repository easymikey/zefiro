use kernel::domain::geometry::Cells;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders},
};

use crate::{
    primitive::{inset::Inset, list_chrome::spaced_title},
    status_line::{self, StatusLineStyle, StatusLineView},
    theme::active_theme::ActiveTheme,
};

const TITLE_CELLS: u16 = 2;
const BORDER_COLUMNS: u16 = 2;

pub(crate) fn pane_block<'a>(title: Option<Line<'a>>, border: Color) -> Block<'a> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(border))
        .padding(Inset::overlay().padding());
    match title {
        Some(title) => block.title(spaced_title(title)),
        None => block,
    }
}

fn title_budget(area: Rect) -> Cells {
    Cells(
        area.width
            .saturating_sub(BORDER_COLUMNS)
            .saturating_sub(TITLE_CELLS),
    )
}

pub(crate) fn pane_title<'a>(
    area: Rect,
    status: StatusLineView<'a>,
    theme: &ActiveTheme<'_>,
) -> Line<'a> {
    status_line::status_line(
        status,
        StatusLineStyle::from_theme(theme),
        title_budget(area),
    )
}

#[cfg(test)]
mod tests {
    use kernel::domain::{index::ViewIndex, playlist::RepeatMode, startup::Shuffle};
    use ratatui::layout::Rect;

    use crate::{
        playlist::chrome::pane_title,
        status_line::{ScanProgress, StatusLineView},
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn status() -> StatusLineView<'static> {
        StatusLineView {
            shuffle: Shuffle::Enabled,
            repeat_mode: RepeatMode::All,
            queue_len: 0,
            position: ViewIndex::new(0),
            total: 0,
            scan: ScanProgress::Done,
            theme_name: "noir",
            sleep_left: None,
        }
    }

    fn title_text(width: u16) -> String {
        let theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        pane_title(Rect::new(0, 0, width, 1), status(), &theme)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let text = title_text(80);
        assert!(text.contains("shuffle on"), "got {text:?}");
        assert!(text.contains("repeat all"), "got {text:?}");
    }

    #[test]
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let text = title_text(24);
        assert!(text.ends_with('…'), "got {text:?}");
    }
}
