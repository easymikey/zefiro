use kernel::domain::geometry::Cells;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Clear, Widget},
};

use crate::{
    overlay::modal::{
        frame::{Modal, ModalAreas, ModalSize},
        metrics::SCROLLBAR_INSET,
    },
    playlist::chrome::pane_block,
    primitive::{
        canvas::Canvas,
        list_chrome::{row_band, scrollbar_column},
        text::truncate,
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModalContainer<'a> {
    Playlist(Rect),
    Modal(&'a [Rect]),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ModalScrollAreas {
    pub(crate) outer: Rect,
    pub rows: Rect,
    pub(crate) content: Rect,
    pub(crate) scrollbar: Rect,
    pub(crate) hint_row: Rect,
}

impl ModalScrollAreas {
    fn empty(outer: Rect) -> Self {
        Self {
            outer,
            rows: Rect::default(),
            content: Rect::default(),
            scrollbar: Rect::default(),
            hint_row: Rect::default(),
        }
    }

    fn frame(self) -> ModalAreas {
        ModalAreas {
            outer: self.outer,
            body: self.content,
            hint_row: self.hint_row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAreas {
    List(ModalScrollAreas),
    Dialog(ModalAreas),
    Banner(Rect),
}

impl OverlayAreas {
    #[must_use]
    pub fn outer(self) -> Rect {
        match self {
            Self::List(areas) => areas.outer,
            Self::Dialog(areas) => areas.outer,
            Self::Banner(rect) => rect,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ModalBorder<'a> {
    pub(crate) area: Rect,
    pub(crate) title: Line<'static>,
    pub(crate) theme: ActiveTheme<'a>,
}

impl ModalBorder<'_> {
    fn block(&self) -> Block<'static> {
        pane_block(
            Some(self.title.clone()),
            self.theme.colors().muted_foreground,
        )
    }

    #[must_use]
    pub(crate) fn areas(&self) -> ModalScrollAreas {
        let inner = self.block().inner(self.area);
        if inner.width == 0 || inner.height == 0 {
            return ModalScrollAreas::empty(self.area);
        }
        scroll_areas(self.area, inner)
    }

    pub(crate) fn paint(&self, buffer: &mut Buffer) {
        Clear.render(self.area, buffer);
        let colors = self.theme.colors();
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.text),
            )
            .render(self.area, buffer);
        self.block().render(self.area, buffer);
    }
}

#[derive(Debug)]
pub(crate) struct ModalPlacement<'a> {
    pub(crate) container: ModalContainer<'a>,
    pub(crate) border_title: Line<'static>,
    pub(crate) modal_title: &'a str,
    pub(crate) content_width: Cells,
    pub(crate) content_rows: Cells,
    pub(crate) hint: Option<Line<'static>>,
    pub(crate) theme: ActiveTheme<'a>,
}

impl<'a> ModalPlacement<'a> {
    fn border(&self, area: Rect) -> ModalBorder<'a> {
        ModalBorder {
            area,
            title: self.border_title.clone(),
            theme: self.theme,
        }
    }

    fn modal(&self) -> Modal<'_> {
        let colors = self.theme.colors();
        Modal {
            title: self.modal_title,
            size: ModalSize::List {
                content_width: self.content_width,
                content_rows: self.content_rows.max(Cells(1)),
            },
            hint: self.hint.clone(),
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalScrollAreas {
        match self.container {
            ModalContainer::Playlist(pane) => self.border(pane).areas(),
            ModalContainer::Modal(avoid) => {
                let modal_frame = self.modal().areas(screen, avoid);
                ModalScrollAreas {
                    hint_row: modal_frame.hint_row,
                    ..scroll_areas(modal_frame.outer, modal_frame.body)
                }
            }
        }
    }

    pub(crate) fn paint(&self, areas: ModalScrollAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        match self.container {
            ModalContainer::Playlist(pane) => self.border(pane).paint(buffer),
            ModalContainer::Modal(_) => self.modal().paint(areas.frame(), buffer),
        }
    }
}

#[must_use]
pub(crate) fn scroll_areas(outer: Rect, inner: Rect) -> ModalScrollAreas {
    let scrollbar = scrollbar_column(outer, inner, SCROLLBAR_INSET);
    ModalScrollAreas {
        outer,
        rows: row_band(outer, inner, scrollbar),
        content: inner,
        scrollbar,
        hint_row: Rect::default(),
    }
}

#[must_use]
pub(crate) fn leading_cells(areas: &ModalScrollAreas) -> Cells {
    Cells(areas.content.x.saturating_sub(areas.rows.x))
}

#[must_use]
fn trailing_cells(areas: &ModalScrollAreas) -> u16 {
    areas.rows.right().saturating_sub(areas.content.right())
}

#[must_use]
pub(crate) fn column_width(areas: &ModalScrollAreas) -> Cells {
    Cells(areas.rows.width.saturating_sub(trailing_cells(areas)))
}

#[must_use]
pub(crate) fn indented(text: &str, lead: Cells, width: Cells) -> String {
    let lead = lead.count();
    let budget = width.count().saturating_sub(lead);
    let fitted = truncate(text, budget);
    format!("{:lead$}{fitted}", "")
}
