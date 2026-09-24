use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Widget},
};

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalBounds, ModalSize, PlacedModal},
    primitive::{
        canvas::Canvas,
        glyphs::TruncateGlyphs,
        inset::Inset,
        list_chrome::{row_band, scrollbar_column, spaced_title},
        text::truncate_to_width,
    },
    theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum OverlayContainer<'a> {
    Pane(Rect),
    Modal { avoid: &'a [Rect] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalScrollAreas {
    pub outer: Rect,
    pub painted: Rect,
    pub rows: Rect,
    pub content: Rect,
    pub scrollbar: Rect,
    pub hint_row: Rect,
}

impl ModalScrollAreas {
    fn empty(outer: Rect) -> Self {
        Self {
            outer,
            painted: outer,
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
            painted: self.painted,
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
    pub fn painted(self) -> Rect {
        match self {
            Self::List(areas) => areas.painted,
            Self::Dialog(areas) => areas.painted,
            Self::Banner(rect) => rect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModalChrome {
    pub(crate) scrollbar_inset: u16,
}

impl Default for ModalChrome {
    fn default() -> Self {
        Self { scrollbar_inset: 2 }
    }
}

#[derive(Debug)]
pub(crate) struct ModalBorder<'a> {
    pub(crate) area: Rect,
    pub(crate) title: Line<'static>,
    pub(crate) inset: Inset,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) chrome: ModalChrome,
}

impl ModalBorder<'_> {
    fn block(&self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(self.theme.frame()))
            .padding(self.inset.padding())
            .title(spaced_title(self.title.clone()))
    }

    #[must_use]
    pub(crate) fn areas(&self) -> ModalScrollAreas {
        let inner = self.block().inner(self.area);
        if inner.width == 0 || inner.height == 0 {
            return ModalScrollAreas::empty(self.area);
        }
        let scrollbar = scrollbar_column(self.area, inner, self.chrome.scrollbar_inset);
        ModalScrollAreas {
            outer: self.area,
            painted: self.area,
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
                    .bg(self.theme.window_bg())
                    .fg(self.theme.text()),
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
    pub(crate) chrome: ModalChrome,
}

impl<'a> ModalPlacement<'a> {
    fn border(&self, area: Rect) -> ModalBorder<'a> {
        ModalBorder {
            area,
            title: self.border_title.clone(),
            inset: self.inset,
            theme: self.theme,
            chrome: self.chrome,
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
            border: self.theme.frame(),
            window_background: self.theme.window_bg(),
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalScrollAreas {
        match self.container {
            OverlayContainer::Pane(pane) => self.border(pane).areas(),
            OverlayContainer::Modal { avoid } => {
                self.scroll_areas(&self.modal().frame(screen, avoid))
            }
        }
    }

    fn scroll_areas(&self, modal_frame: &ModalAreas) -> ModalScrollAreas {
        let scrollbar = scrollbar_column(
            modal_frame.outer,
            modal_frame.body,
            self.chrome.scrollbar_inset,
        );
        ModalScrollAreas {
            outer: modal_frame.outer,
            painted: modal_frame.painted,
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
pub(crate) fn lead_cells(areas: &ModalScrollAreas) -> u16 {
    areas.content.x.saturating_sub(areas.rows.x)
}

#[must_use]
fn trail_cells(areas: &ModalScrollAreas) -> u16 {
    areas.rows.right().saturating_sub(areas.content.right())
}

#[must_use]
pub(crate) fn column_width(areas: &ModalScrollAreas) -> u16 {
    areas.rows.width.saturating_sub(trail_cells(areas))
}

#[must_use]
pub(crate) fn led(text: &str, lead: u16, width: u16) -> String {
    let lead = usize::from(lead);
    let budget = usize::from(width).saturating_sub(lead);
    let fitted = truncate_to_width(text, budget, TruncateGlyphs::default());
    format!("{:lead$}{fitted}", "")
}
