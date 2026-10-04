use std::iter::once;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget},
};

use crate::primitive::{glyphs, span::text};

const TITLE_SPACE: &str = " ";

#[must_use]
pub(crate) fn spaced_title<'a>(title: Line<'a>) -> Line<'a> {
    let space = || Span::from(text(TITLE_SPACE));
    Line::from_iter(once(space()).chain(title.spans).chain(once(space())))
}

#[must_use]
pub(crate) fn row_band(outer: Rect, content: Rect, scrollbar: Rect) -> Rect {
    let left = outer.x.saturating_add(1).min(content.x);
    let right = if scrollbar.width == 0 {
        outer.right().saturating_sub(1)
    } else {
        scrollbar.x
    }
    .max(content.right());
    Rect {
        x: left,
        y: content.y,
        width: right.saturating_sub(left),
        height: content.height,
    }
}

#[must_use]
pub(crate) fn scrollbar_column(outer: Rect, content: Rect, inset: u16) -> Rect {
    Rect {
        x: outer.x + outer.width.saturating_sub(inset),
        y: content.y,
        width: 1,
        height: content.height,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollbarTrack {
    pub total: usize,
    pub offset: usize,
    pub viewport: usize,
    pub thumb: Color,
    pub track: Color,
}

pub(crate) fn paint_scrollbar(column: Rect, bar: ScrollbarTrack, buffer: &mut Buffer) {
    if bar.total <= bar.viewport {
        return;
    }
    let thumb = Style::default().fg(bar.thumb);
    let widget = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some(glyphs::scrollbar::UP))
        .end_symbol(Some(glyphs::scrollbar::DOWN))
        .track_symbol(Some(glyphs::scrollbar::TRACK))
        .thumb_symbol(glyphs::scrollbar::THUMB)
        .begin_style(thumb)
        .end_style(thumb)
        .thumb_style(thumb)
        .track_style(Style::default().fg(bar.track));
    let mut position = ScrollbarState::new(bar.total)
        .position(bar.offset)
        .viewport_content_length(bar.viewport);
    StatefulWidget::render(widget, column, buffer, &mut position);
}

#[must_use]
pub(crate) fn scroll_offset(selected: usize, total: usize, height: usize) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    let max_offset = total - height;
    selected.saturating_sub(height - 1).min(max_offset)
}
