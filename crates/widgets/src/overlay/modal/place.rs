use kernel::domain::geometry::Cells;
use ratatui::{
    layout::{Constraint, Rect},
    widgets::{Block, Padding},
};

const BORDER_CELLS: Cells = Cells(2);
const PADDING_X: Cells = Cells(1);
const PADDING_TOP: Cells = Cells(0);
const HINT_ROWS: Cells = Cells(1);
pub(crate) const LIST_SCREEN_MARGIN: Cells = Cells(2);
pub(crate) const DIALOG_SCREEN_MARGIN: Cells = Cells(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hint {
    Present,
    Absent,
}

fn hint_rows_for(hint: Hint) -> Cells {
    match hint {
        Hint::Present => HINT_ROWS,
        Hint::Absent => Cells(0),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentSize {
    pub(crate) min_width: Cells,
    pub(crate) content_width: Cells,
    pub(crate) content_rows: Cells,
    pub(crate) hint: Hint,
    pub(crate) screen_margin: Cells,
}

pub(crate) fn content_size(area: Rect, size: ContentSize) -> PlacedSize {
    let height = framed_rows(size.content_rows, size.hint);
    let width = size
        .content_width
        .max(size.min_width)
        .0
        .saturating_add(PADDING_X.0 * 2)
        .saturating_add(BORDER_CELLS.0);

    let max_width = area.width.saturating_sub(size.screen_margin.0);
    let max_height = area.height.saturating_sub(size.screen_margin.0);
    PlacedSize {
        width: Cells(width.min(max_width).min(area.width)),
        height: Cells(height.0.min(max_height).min(area.height)),
    }
}

fn framed_rows(content_rows: Cells, hint: Hint) -> Cells {
    Cells(
        content_rows
            .0
            .saturating_add(PADDING_TOP.0)
            .saturating_add(hint_rows_for(hint).0)
            .saturating_add(BORDER_CELLS.0),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameWidth {
    pub(crate) bounds: Rect,
    pub(crate) content_rows: Cells,
}

pub(crate) fn width(hint: Hint, spec: FrameWidth) -> Rect {
    let height = framed_rows(spec.content_rows, hint)
        .0
        .min(spec.bounds.height);
    Rect {
        x: spec.bounds.x,
        y: spec.bounds.y,
        width: spec.bounds.width,
        height,
    }
}

#[must_use]
pub(crate) fn list_capacity(area: Rect, hint: Hint) -> PlacedSize {
    let max_width = area.width.saturating_sub(LIST_SCREEN_MARGIN.0);
    let max_height = area.height.saturating_sub(LIST_SCREEN_MARGIN.0);
    let content_width = max_width
        .saturating_sub(PADDING_X.0 * 2)
        .saturating_sub(BORDER_CELLS.0);
    let content_rows = max_height
        .saturating_sub(PADDING_TOP.0)
        .saturating_sub(hint_rows_for(hint).0)
        .saturating_sub(BORDER_CELLS.0);
    PlacedSize {
        width: Cells(content_width),
        height: Cells(content_rows),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacedSize {
    pub(crate) width: Cells,
    pub(crate) height: Cells,
}

fn centered(area: Rect, size: PlacedSize) -> Rect {
    area.centered(
        Constraint::Length(size.width.0),
        Constraint::Length(size.height.0),
    )
}

pub(crate) fn intersects_any(rect: Rect, avoid: &[Rect]) -> bool {
    avoid.iter().any(|region| rect.intersects(*region))
}

pub(crate) fn place(area: Rect, size: PlacedSize, avoid: &[Rect]) -> Rect {
    let candidate = centered(area, size);
    if !intersects_any(candidate, avoid) {
        return candidate;
    }

    let colliding = || {
        avoid
            .iter()
            .copied()
            .filter(|region| region.intersects(candidate))
    };

    let below = colliding()
        .map(Rect::bottom)
        .max()
        .filter(|y| y.saturating_add(size.height.0) <= area.bottom())
        .map(|y| Rect { y, ..candidate });
    let right = colliding()
        .map(Rect::right)
        .max()
        .filter(|x| x.saturating_add(size.width.0) <= area.right())
        .map(|x| Rect { x, ..candidate });

    [below, right]
        .into_iter()
        .flatten()
        .find(|shifted| !intersects_any(*shifted, avoid))
        .unwrap_or(candidate)
}

pub(crate) fn padded_content(outer: Rect) -> Rect {
    let border = BORDER_CELLS.0 / 2;
    Block::default()
        .padding(Padding::new(
            border + PADDING_X.0,
            border + PADDING_X.0,
            border + PADDING_TOP.0,
            border,
        ))
        .inner(outer)
}

pub(crate) fn split_hint_row(content: Rect, hint: Hint) -> (Rect, Rect) {
    if matches!(hint, Hint::Absent) || content.height == 0 {
        let hint_row = Rect {
            y: content.y + content.height,
            height: 0,
            ..content
        };
        return (content, hint_row);
    }
    let body = Rect {
        height: content.height - 1,
        ..content
    };
    let hint_row = Rect {
        y: content.y + content.height - 1,
        height: 1,
        ..content
    };
    (body, hint_row)
}
