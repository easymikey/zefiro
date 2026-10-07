use kernel::domain::geometry::Cells;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Clear, Widget},
};

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalSize},
    playlist::chrome::pane_block,
    primitive::{
        canvas::Canvas,
        list_chrome::{ScrollAreas, scroll_areas},
        truncate::truncate,
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModalContainer<'a> {
    Playlist(Rect),
    Floating(&'a [Rect]),
}

impl From<ScrollAreas> for ModalAreas {
    fn from(areas: ScrollAreas) -> Self {
        Self {
            outer: areas.outer,
            body: areas.content,
            hint_row: areas.hint_row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAreas {
    List(ScrollAreas),
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
    pub(crate) title: Line<'a>,
    pub(crate) theme: ActiveTheme<'a>,
}

impl ModalBorder<'_> {
    #[must_use]
    pub(crate) fn areas(&self) -> ScrollAreas {
        let inner =
            pane_block(None, self.theme.colors().muted_foreground).inner(self.area);
        if inner.width == 0 || inner.height == 0 {
            return ScrollAreas::empty(self.area);
        }
        scroll_areas(self.area, inner)
    }

    pub(crate) fn paint(self, buffer: &mut Buffer) {
        Clear.render(self.area, buffer);
        let colors = self.theme.colors();
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.foreground),
            )
            .render(self.area, buffer);
        pane_block(Some(self.title), colors.muted_foreground).render(self.area, buffer);
    }
}

#[derive(Debug)]
pub(crate) struct ModalPlacement<'a> {
    pub(crate) container: ModalContainer<'a>,
    pub(crate) border_title: Line<'a>,
    pub(crate) modal_title: &'a str,
    pub(crate) content_width: Cells,
    pub(crate) content_rows: Cells,
    pub(crate) theme: ActiveTheme<'a>,
}

impl<'a> ModalPlacement<'a> {
    fn border(&self, area: Rect) -> ModalBorder<'a> {
        ModalBorder {
            area,
            title: Line::default(),
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
            hint: None,
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ScrollAreas {
        match self.container {
            ModalContainer::Playlist(pane) => self.border(pane).areas(),
            ModalContainer::Floating(avoid) => {
                let areas = self.modal().areas(screen, avoid);
                ScrollAreas {
                    hint_row: areas.hint_row,
                    ..scroll_areas(areas.outer, areas.body)
                }
            }
        }
    }

    pub(crate) fn paint(self, areas: ScrollAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        match self.container {
            ModalContainer::Playlist(pane) => ModalBorder {
                area: pane,
                title: self.border_title,
                theme: self.theme,
            }
            .paint(buffer),
            ModalContainer::Floating(_) => {
                self.modal().paint(ModalAreas::from(areas), buffer);
            }
        }
    }
}

#[must_use]
pub(crate) fn leading_cells(areas: &ScrollAreas) -> Cells {
    Cells(areas.content.x.saturating_sub(areas.rows.x))
}

#[must_use]
fn trailing_cells(areas: &ScrollAreas) -> u16 {
    areas.rows.right().saturating_sub(areas.content.right())
}

#[must_use]
pub(crate) fn column_width(areas: &ScrollAreas) -> Cells {
    Cells(areas.rows.width.saturating_sub(trailing_cells(areas)))
}

#[must_use]
pub(crate) fn indented(text: &str, lead_width: Cells, width: Cells) -> String {
    let lead = lead_width.count();
    let budget = width.count().saturating_sub(lead);
    let fitted = truncate(text, budget);
    format!("{:lead$}{fitted}", "")
}
