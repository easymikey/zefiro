use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Widget},
};

use crate::{
    overlay::modal::{
        frame::{Modal, ModalAreas, ModalBounds, ModalSize, PlacedModal},
        metrics::SCROLLBAR_INSET,
    },
    primitive::{
        canvas::Canvas,
        inset::Inset,
        list_chrome::{row_band, scrollbar_column, spaced_title},
        text::truncate,
    },
    theme::{ActiveTheme, Role},
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum OverlayContainer<'a> {
    Pane(Rect),
    Modal { avoid: &'a [Rect] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalScrollAreas {
    pub outer: Rect,
    pub rows: Rect,
    pub content: Rect,
    pub scrollbar: Rect,
    pub hint_row: Rect,
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
    pub(crate) inset: Inset,
    pub(crate) theme: ActiveTheme<'a>,
}

impl ModalBorder<'_> {
    fn block(&self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(self.theme.role(Role::Frame)))
            .padding(self.inset.padding())
            .title(spaced_title(self.title.clone()))
    }

    #[must_use]
    pub(crate) fn areas(&self) -> ModalScrollAreas {
        let inner = self.block().inner(self.area);
        if inner.width == 0 || inner.height == 0 {
            return ModalScrollAreas::empty(self.area);
        }
        let scrollbar = scrollbar_column(self.area, inner, SCROLLBAR_INSET);
        ModalScrollAreas {
            outer: self.area,
            rows: row_band(self.area, inner, scrollbar),
            content: inner,
            scrollbar,
            hint_row: Rect::default(),
        }
    }

    pub(crate) fn paint(&self, buffer: &mut Buffer) {
        Clear.render(self.area, buffer);
        Block::new()
            .style(
                Style::default()
                    .bg(self.theme.role(Role::WindowBackground))
                    .fg(self.theme.role(Role::Text)),
            )
            .render(self.area, buffer);
        self.block().render(self.area, buffer);
    }
}

#[derive(Debug)]
pub(crate) struct ModalPlacement<'a> {
    pub(crate) container: OverlayContainer<'a>,
    pub(crate) inset: Inset,
    pub(crate) border_title: Line<'static>,
    pub(crate) modal_title: &'a str,
    pub(crate) content_width: u16,
    pub(crate) content_rows: u16,
    pub(crate) hint: Option<Line<'static>>,
    pub(crate) theme: ActiveTheme<'a>,
}

impl<'a> ModalPlacement<'a> {
    fn border(&self, area: Rect) -> ModalBorder<'a> {
        ModalBorder {
            area,
            title: self.border_title.clone(),
            inset: self.inset,
            theme: self.theme,
        }
    }

    fn modal(&self) -> Modal<'_> {
        Modal {
            title: self.modal_title,
            size: ModalSize::List {
                content_width: self.content_width,
                content_rows: self.content_rows.max(1),
            },
            hint: self.hint.clone(),
            border: self.theme.role(Role::Frame),
            window_background: self.theme.role(Role::WindowBackground),
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalScrollAreas {
        match self.container {
            OverlayContainer::Pane(pane) => self.border(pane).areas(),
            OverlayContainer::Modal { avoid } => {
                self.scroll_areas(&self.modal().areas(screen, avoid))
            }
        }
    }

    fn scroll_areas(&self, modal_frame: &ModalAreas) -> ModalScrollAreas {
        let scrollbar =
            scrollbar_column(modal_frame.outer, modal_frame.body, SCROLLBAR_INSET);
        ModalScrollAreas {
            outer: modal_frame.outer,
            rows: row_band(modal_frame.outer, modal_frame.body, scrollbar),
            content: modal_frame.body,
            scrollbar,
            hint_row: modal_frame.hint_row,
        }
    }

    pub(crate) fn paint(&self, areas: ModalScrollAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        match self.container {
            OverlayContainer::Pane(pane) => self.border(pane).paint(buffer),
            OverlayContainer::Modal { avoid } => self.modal().paint(
                PlacedModal {
                    areas: areas.frame(),
                    bounds: ModalBounds { area, avoid },
                },
                buffer,
            ),
        }
    }
}

#[must_use]
pub(crate) fn leading_cells(areas: &ModalScrollAreas) -> u16 {
    areas.content.x.saturating_sub(areas.rows.x)
}

#[must_use]
fn trailing_cells(areas: &ModalScrollAreas) -> u16 {
    areas.rows.right().saturating_sub(areas.content.right())
}

#[must_use]
pub(crate) fn column_width(areas: &ModalScrollAreas) -> u16 {
    areas.rows.width.saturating_sub(trailing_cells(areas))
}

#[must_use]
pub(crate) fn indented(text: &str, lead: u16, width: u16) -> String {
    let lead = usize::from(lead);
    let budget = usize::from(width).saturating_sub(lead);
    let fitted = truncate(text, budget);
    format!("{:lead$}{fitted}", "")
}
