use kernel::domain::geometry::Cells;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Widget},
};

use crate::overlay::modal::place::{
    ContentSize,
    DIALOG_SCREEN_MARGIN,
    FullWidth,
    Hint,
    LIST_SCREEN_MARGIN,
    frame_size,
    padded_content,
    place,
    split_hint_row,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSize {
    List {
        content_width: Cells,
        content_rows: Cells,
    },
    Dialog {
        min_width: Cells,
        content_width: Cells,
        content_rows: Cells,
    },
    FullWidth(FullWidth),
}

#[derive(Debug)]
pub(crate) struct Modal<'a> {
    pub(crate) title: &'a str,
    pub(crate) size: ModalSize,
    pub(crate) hint: Option<Line<'a>>,
    pub(crate) border: Color,
    pub(crate) window_background: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModalBounds<'a> {
    pub(crate) area: Rect,
    pub(crate) avoid: &'a [Rect],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalAreas {
    pub(crate) outer: Rect,
    pub(crate) body: Rect,
    pub(crate) hint_row: Rect,
}

impl<'a> Modal<'a> {
    #[must_use]
    pub(crate) fn areas(&self, area: Rect, avoid: &[Rect]) -> ModalAreas {
        let hint = if self.hint.is_some() {
            Hint::Shown
        } else {
            Hint::Hidden
        };
        let outer = self.outer(ModalBounds { area, avoid }, hint);
        let content = padded_content(outer);
        let (body, hint_row) = split_hint_row(content, hint);
        ModalAreas {
            outer,
            body,
            hint_row,
        }
    }

    fn outer(&self, bounds: ModalBounds<'_>, hint: Hint) -> Rect {
        let size = match self.size {
            ModalSize::List {
                content_width,
                content_rows,
            } => ContentSize {
                min_width: Cells(0),
                content_width,
                content_rows,
                hint,
                screen_margin_width: LIST_SCREEN_MARGIN,
            },
            ModalSize::Dialog {
                min_width,
                content_width,
                content_rows,
            } => ContentSize {
                min_width,
                content_width,
                content_rows,
                hint,
                screen_margin_width: DIALOG_SCREEN_MARGIN,
            },
            ModalSize::FullWidth(full_width) => return full_width.outer(hint),
        };
        place(bounds.area, frame_size(bounds.area, size), bounds.avoid)
    }

    pub(crate) fn paint(&self, modal_areas: ModalAreas, buffer: &mut Buffer) {
        Clear.render(modal_areas.outer, buffer);
        Block::new()
            .style(Style::default().bg(self.window_background))
            .render(modal_areas.outer, buffer);

        let title = format!(" {} ", self.title);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.border))
            .title(title)
            .title_style(Style::default().fg(self.border))
            .render(modal_areas.outer, buffer);

        if let Some(hint) = &self.hint
            && modal_areas.hint_row.height > 0
        {
            hint.render(modal_areas.hint_row, buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Cells;
    use ratatui::{layout::Rect, style::Color, text::Line};
    use rstest::rstest;

    use crate::overlay::modal::{
        frame::{Modal, ModalSize},
        place::{Hint, list_capacity},
    };

    fn area(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    fn list(content_width: u16, content_rows: u16) -> ModalSize {
        ModalSize::List {
            content_width: Cells(content_width),
            content_rows: Cells(content_rows),
        }
    }

    fn modal(size: ModalSize, hint: Hint) -> Modal<'static> {
        Modal {
            title: "T",
            size,
            hint: match hint {
                Hint::Shown => Some(Line::from("hint")),
                Hint::Hidden => None,
            },
            border: Color::Reset,
            window_background: Color::Reset,
        }
    }

    struct ModalRow {
        name: &'static str,
        size: ModalSize,
        hint: Hint,
        screen: Rect,
    }

    #[rstest]
    #[case::a_dialog_narrower_than_its_minimum(ModalRow {
        name: "dialog_min_width",
        size: ModalSize::Dialog { min_width: Cells(40), content_width: Cells(5), content_rows: Cells(1) },
        hint: Hint::Hidden,
        screen: area(80, 24),
    })]
    #[case::a_dialog_larger_than_the_screen(ModalRow {
        name: "dialog_clamped",
        size: ModalSize::Dialog { min_width: Cells(200), content_width: Cells(200), content_rows: Cells(3) },
        hint: Hint::Shown,
        screen: area(80, 24),
    })]
    fn a_modal_frames_itself_inside_the_screen(#[case] row: ModalRow) {
        let frame = modal(row.size, row.hint).areas(row.screen, &[]);

        assert!(frame.outer.width <= row.screen.width);
        assert!(frame.outer.height <= row.screen.height);
        assert!(frame.body.x > frame.outer.x);
        assert!(frame.body.x + frame.body.width < frame.outer.x + frame.outer.width);
        assert_eq!(frame.body.y + frame.body.height, frame.hint_row.y);
        assert!(
            frame.hint_row.y + frame.hint_row.height
                < frame.outer.y + frame.outer.height
        );
        let hint_present = if frame.hint_row.height > 0 {
            Hint::Shown
        } else {
            Hint::Hidden
        };
        assert_eq!(hint_present, row.hint);

        insta::with_settings!({ snapshot_suffix => row.name }, {
            insta::assert_debug_snapshot!(frame);
        });
    }

    #[test]
    fn list_capacity_matches_what_frame_clamps_a_huge_list_to() {
        let screen = area(80, 24);
        let capacity = list_capacity(screen, Hint::Shown);
        let frame = modal(list(200, 200), Hint::Shown).areas(screen, &[]);
        assert_eq!(
            (frame.body.width, frame.body.height),
            (capacity.width.0, capacity.height.0)
        );
    }

    #[test]
    fn frame_shifts_below_a_top_left_cover_rect_it_would_otherwise_overlap() {
        let modal = Modal {
            title: "T",
            size: ModalSize::List {
                content_width: Cells(20),
                content_rows: Cells(5),
            },
            hint: None,
            border: Color::Reset,
            window_background: Color::Reset,
        };
        let screen = area(80, 24);
        let centered_outer = modal.areas(screen, &[]).outer;
        let cover_area = Rect {
            x: 0,
            y: 0,
            width: centered_outer.x + centered_outer.width / 2,
            height: centered_outer.y + centered_outer.height / 2,
        };
        assert!(cover_area.intersects(centered_outer));

        let frame = modal.areas(screen, &[cover_area]);
        assert!(
            !frame.outer.intersects(cover_area),
            "modal must move clear of the cover rect, got {:?} vs cover {:?}",
            frame.outer,
            cover_area
        );
        assert!(
            frame.outer.y >= cover_area.y + cover_area.height
                || frame.outer.x >= cover_area.x + cover_area.width,
            "modal must shift below or right of the cover, not sideways past it \
             some other way, got {:?}",
            frame.outer
        );
        assert!(
            frame.outer.x + frame.outer.width <= screen.x + screen.width
                && frame.outer.y + frame.outer.height <= screen.y + screen.height,
            "shifted modal must still fit on screen, got {:?}",
            frame.outer
        );
    }

    #[test]
    fn frame_keeps_centered_when_it_cannot_fit_anywhere_clear_of_avoid() {
        let modal = Modal {
            title: "T",
            size: ModalSize::List {
                content_width: Cells(20),
                content_rows: Cells(5),
            },
            hint: None,
            border: Color::Reset,
            window_background: Color::Reset,
        };
        let screen = area(80, 24);
        let centered_outer = modal.areas(screen, &[]).outer;
        let covers_everything = Rect {
            x: 0,
            y: 0,
            width: screen.width,
            height: screen.height,
        };
        let frame = modal.areas(screen, &[covers_everything]);
        assert_eq!(frame.outer, centered_outer);
    }
}
