use ratatui::{
    layout::{Constraint, Rect},
    widgets::{Block, Padding},
};

use crate::overlay::modal::frame::{Hint, ModalLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentSize {
    pub(crate) min_width: u16,
    pub(crate) content_width: u16,
    pub(crate) content_rows: u16,
    pub(crate) hint: Hint,
    pub(crate) screen_margin: u16,
}

pub(crate) fn content_dimensions(
    area: Rect,
    layout: ModalLayout,
    size: ContentSize,
) -> (u16, u16) {
    let hint_rows = layout.hint_rows_for(size.hint);
    let height = size
        .content_rows
        .saturating_add(layout.padding_top)
        .saturating_add(hint_rows)
        .saturating_add(layout.border_cells);
    let width = size
        .content_width
        .max(size.min_width)
        .saturating_add(layout.padding_x * 2)
        .saturating_add(layout.border_cells);

    let max_width = area.width.saturating_sub(size.screen_margin);
    let max_height = area.height.saturating_sub(size.screen_margin);
    (
        width.min(max_width).min(area.width),
        height.min(max_height).min(area.height),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameWidthBox {
    pub(crate) bounds: Rect,
    pub(crate) content_rows: u16,
}

pub(crate) fn anchored_frame(
    layout: ModalLayout,
    hint: Hint,
    spec: FrameWidthBox,
) -> Rect {
    let hint_rows = layout.hint_rows_for(hint);
    let height = spec
        .content_rows
        .saturating_add(layout.padding_top)
        .saturating_add(hint_rows)
        .saturating_add(layout.border_cells)
        .min(spec.bounds.height);
    Rect {
        x: spec.bounds.x,
        y: spec.bounds.y,
        width: spec.bounds.width,
        height,
    }
}

#[must_use]
pub(crate) fn list_capacity(area: Rect, hint: Hint) -> (u16, u16) {
    let layout = ModalLayout::default();
    let hint_rows = layout.hint_rows_for(hint);
    let max_width = area.width.saturating_sub(layout.list_screen_margin);
    let max_height = area.height.saturating_sub(layout.list_screen_margin);
    let content_width = max_width
        .saturating_sub(layout.padding_x * 2)
        .saturating_sub(layout.border_cells);
    let content_rows = max_height
        .saturating_sub(layout.padding_top)
        .saturating_sub(hint_rows)
        .saturating_sub(layout.border_cells);
    (content_width, content_rows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoxSize {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

fn centered(area: Rect, size: BoxSize) -> Rect {
    area.centered(
        Constraint::Length(size.width),
        Constraint::Length(size.height),
    )
}

pub(crate) fn intersects_any(rect: Rect, avoid: &[Rect]) -> bool {
    avoid.iter().any(|region| rect.intersects(*region))
}

pub(crate) fn place(area: Rect, size: BoxSize, avoid: &[Rect]) -> Rect {
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
        .filter(|y| y.saturating_add(size.height) <= area.bottom())
        .map(|y| Rect { y, ..candidate });
    let right = colliding()
        .map(Rect::right)
        .max()
        .filter(|x| x.saturating_add(size.width) <= area.right())
        .map(|x| Rect { x, ..candidate });

    [below, right]
        .into_iter()
        .flatten()
        .find(|shifted| !intersects_any(*shifted, avoid))
        .unwrap_or(candidate)
}

pub(crate) fn padded_content(outer: Rect, layout: ModalLayout) -> Rect {
    let border = layout.border_cells / 2;
    Block::default()
        .padding(Padding::new(
            border + layout.padding_x,
            border + layout.padding_x,
            border + layout.padding_top,
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
