use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};

use crate::overlay::modal::place::{
    BoxSize,
    ContentSize,
    FrameWidthBox,
    anchored_frame,
    content_dimensions,
    padded_content,
    place,
    split_hint_row,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSize {
    List {
        content_width: u16,
        content_rows: u16,
    },
    Dialog {
        min_width: u16,
        content_width: u16,
        content_lines: u16,
    },
    FrameWidth {
        bounds: Rect,
        content_rows: u16,
    },
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
pub(crate) struct ModalBounds<'a> {
    pub(crate) area: Rect,
    pub(crate) avoid: &'a [Rect],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalAreas {
    pub outer: Rect,
    pub body: Rect,
    pub hint_row: Rect,
    pub painted: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacedModal<'a> {
    pub(crate) areas: ModalAreas,
    pub(crate) bounds: ModalBounds<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalGlyphs {
    pub(crate) title_prefix: &'static str,
    pub(crate) title_suffix: &'static str,
    pub(crate) shade: &'static str,
}

impl Default for ModalGlyphs {
    fn default() -> Self {
        Self {
            title_prefix: " ",
            title_suffix: " ",
            shade: "\u{2591}",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalLayout {
    pub(crate) border_cells: u16,
    pub(crate) padding_x: u16,
    pub(crate) padding_top: u16,
    pub(crate) hint_rows: u16,
    pub(crate) list_screen_margin: u16,
    pub(crate) dialog_screen_margin: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hint {
    Present,
    Absent,
}

impl ModalLayout {
    pub(crate) fn hint_rows_for(self, hint: Hint) -> u16 {
        match hint {
            Hint::Present => self.hint_rows,
            Hint::Absent => 0,
        }
    }
}

impl Default for ModalLayout {
    fn default() -> Self {
        Self {
            border_cells: 2,
            padding_x: 1,
            padding_top: 0,
            hint_rows: 1,
            list_screen_margin: 2,
            dialog_screen_margin: 4,
        }
    }
}

impl<'a> Modal<'a> {
    #[must_use]
    pub(crate) fn frame(&self, area: Rect, avoid: &[Rect]) -> ModalAreas {
        let hint = if self.hint.is_some() {
            Hint::Present
        } else {
            Hint::Absent
        };
        let outer = self.outer(ModalBounds { area, avoid }, hint);
        let content = padded_content(outer, ModalLayout::default());
        let (body, hint_row) = split_hint_row(content, hint);
        ModalAreas {
            outer,
            body,
            hint_row,
            painted: outer,
        }
    }

    fn outer(&self, bounds: ModalBounds<'_>, hint: Hint) -> Rect {
        let layout = ModalLayout::default();
        match self.size {
            ModalSize::List {
                content_width,
                content_rows,
            } => {
                let (width, height) = content_dimensions(
                    bounds.area,
                    layout,
                    ContentSize {
                        min_width: 0,
                        content_width,
                        content_rows,
                        hint,
                        screen_margin: layout.list_screen_margin,
                    },
                );
                place(bounds.area, BoxSize { width, height }, bounds.avoid)
            }
            ModalSize::Dialog {
                min_width,
                content_width,
                content_lines,
            } => {
                let (width, height) = content_dimensions(
                    bounds.area,
                    layout,
                    ContentSize {
                        min_width,
                        content_width,
                        content_rows: content_lines,
                        hint,
                        screen_margin: layout.dialog_screen_margin,
                    },
                );
                place(bounds.area, BoxSize { width, height }, bounds.avoid)
            }
            ModalSize::FrameWidth {
                bounds: frame_bounds,
                content_rows,
            } => anchored_frame(
                layout,
                hint,
                FrameWidthBox {
                    bounds: frame_bounds,
                    content_rows,
                },
            ),
        }
    }

    pub(crate) fn paint(&self, placed: PlacedModal<'_>, buffer: &mut Buffer) {
        let modal_frame = placed.areas;
        Clear.render(modal_frame.outer, buffer);
        Block::new()
            .style(Style::default().bg(self.window_background))
            .render(modal_frame.outer, buffer);

        let glyphs = ModalGlyphs::default();
        let title = format!(
            "{}{}{}",
            glyphs.title_prefix, self.title, glyphs.title_suffix
        );
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.border))
            .title(title)
            .title_style(Style::default().fg(self.border))
            .render(modal_frame.outer, buffer);

        if let Some(hint) = self.hint.clone()
            && modal_frame.hint_row.height > 0
        {
            Paragraph::new(hint).render(modal_frame.hint_row, buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Rect, style::Color, text::Line};
    use rstest::rstest;

    use crate::overlay::modal::{
        frame::{Hint, Modal, ModalSize},
        place::list_capacity,
    };

    fn area(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    fn list(content_width: u16, content_rows: u16) -> ModalSize {
        ModalSize::List {
            content_width,
            content_rows,
        }
    }

    fn modal(size: ModalSize, hint: Hint) -> Modal<'static> {
        Modal {
            title: "T",
            size,
            hint: match hint {
                Hint::Present => Some(Line::from("hint")),
                Hint::Absent => None,
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
    #[case::a_list(ModalRow { name: "list", size: list(20, 5), hint: Hint::Absent, screen: area(80, 24) })]
    #[case::a_list_with_a_hint(ModalRow { name: "list_hint", size: list(20, 5), hint: Hint::Present, screen: area(80, 24) })]
    #[case::a_list_larger_than_the_screen(ModalRow { name: "list_clamped", size: list(200, 200), hint: Hint::Present, screen: area(80, 24) })]
    #[case::a_list_on_a_tiny_screen(ModalRow { name: "list_tiny", size: list(200, 200), hint: Hint::Absent, screen: area(30, 10) })]
    #[case::a_dialog(ModalRow {
        name: "dialog",
        size: ModalSize::Dialog { min_width: 24, content_width: 20, content_lines: 3 },
        hint: Hint::Present,
        screen: area(80, 24),
    })]
    #[case::a_dialog_narrower_than_its_minimum(ModalRow {
        name: "dialog_min_width",
        size: ModalSize::Dialog { min_width: 40, content_width: 5, content_lines: 1 },
        hint: Hint::Absent,
        screen: area(80, 24),
    })]
    #[case::a_dialog_larger_than_the_screen(ModalRow {
        name: "dialog_clamped",
        size: ModalSize::Dialog { min_width: 200, content_width: 200, content_lines: 3 },
        hint: Hint::Present,
        screen: area(80, 24),
    })]
    fn a_modal_frames_itself_inside_the_screen(#[case] row: ModalRow) {
        let frame = modal(row.size, row.hint).frame(row.screen, &[]);

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
            Hint::Present
        } else {
            Hint::Absent
        };
        assert_eq!(hint_present, row.hint);

        insta::with_settings!({ snapshot_suffix => row.name }, {
            insta::assert_debug_snapshot!(frame);
        });
    }

    #[test]
    fn list_capacity_matches_what_frame_clamps_a_huge_list_to() {
        let screen = area(80, 24);
        let (capacity_width, capacity_rows) = list_capacity(screen, Hint::Present);
        let frame = modal(list(200, 200), Hint::Present).frame(screen, &[]);
        assert_eq!(
            (frame.body.width, frame.body.height),
            (capacity_width, capacity_rows)
        );
    }

    #[test]
    fn frame_shifts_below_a_top_left_cover_rect_it_would_otherwise_overlap() {
        let modal = Modal {
            title: "T",
            size: ModalSize::List {
                content_width: 20,
                content_rows: 5,
            },
            hint: None,
            border: Color::Reset,
            window_background: Color::Reset,
        };
        let screen = area(80, 24);
        let centered_outer = modal.frame(screen, &[]).outer;
        let cover = Rect {
            x: 0,
            y: 0,
            width: centered_outer.x + centered_outer.width / 2,
            height: centered_outer.y + centered_outer.height / 2,
        };
        assert!(cover.intersects(centered_outer));

        let frame = modal.frame(screen, &[cover]);
        assert!(
            !frame.outer.intersects(cover),
            "modal must move clear of the cover rect, got {:?} vs cover {:?}",
            frame.outer,
            cover
        );
        assert!(
            frame.outer.y >= cover.y + cover.height
                || frame.outer.x >= cover.x + cover.width,
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
                content_width: 20,
                content_rows: 5,
            },
            hint: None,
            border: Color::Reset,
            window_background: Color::Reset,
        };
        let screen = area(80, 24);
        let centered_outer = modal.frame(screen, &[]).outer;
        let covers_everything = Rect {
            x: 0,
            y: 0,
            width: screen.width,
            height: screen.height,
        };
        let frame = modal.frame(screen, &[covers_everything]);
        assert_eq!(frame.outer, centered_outer);
    }
}
