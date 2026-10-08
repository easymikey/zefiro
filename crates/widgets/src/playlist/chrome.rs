use kernel::domain::geometry::Cells;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders},
};

use crate::{
    primitive::{inset::Inset, list_chrome::spaced_title},
    status_line::{self, StatusLineView},
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

pub(crate) fn title_budget(area: Rect) -> Cells {
    Cells(
        area.width
            .saturating_sub(BORDER_COLUMNS)
            .saturating_sub(TITLE_CELLS),
    )
}

pub(crate) fn pane_title<'a>(
    area: Rect,
    status_line_view: StatusLineView<'a>,
    theme: &ActiveTheme<'_>,
) -> Line<'a> {
    status_line::status_line(status_line_view, &theme.colors(), title_budget(area))
}

#[cfg(test)]
mod tests {
    use kernel::domain::{index::ViewIndex, playlist::RepeatMode, startup::Shuffle};
    use ratatui::layout::Rect;

    use crate::{
        playlist::chrome::pane_title,
        status_line::StatusLineView,
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn status() -> StatusLineView<'static> {
        StatusLineView {
            shuffle: Shuffle::On,
            repeat_mode: RepeatMode::All,
            queue_len: 0,
            selected: ViewIndex::new(0),
            playlist_len: 0,
            scan_status: kernel::domain::model::ScanStatus::Idle,
            scanning_label: "Scanning…",
            theme_name: "noir",
            remaining: None,
            servers: &[],
            catalog_name: &kernel::domain::catalog::CatalogName::Local,
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
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let text = title_text(24);
        assert!(text.ends_with('…'), "got {text:?}");
    }
}
