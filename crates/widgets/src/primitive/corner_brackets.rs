use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};

use crate::primitive::glyphs::CornerGlyphs;

#[must_use]
pub(crate) fn expand(area: Rect, margin: u16) -> Rect {
    Rect {
        x: area.x.saturating_sub(margin),
        y: area.y.saturating_sub(margin),
        width: area.width.saturating_add(margin * 2),
        height: area.height.saturating_add(margin * 2),
    }
}

#[derive(Debug)]
pub(crate) struct CornerRing {
    pub(crate) color: Color,
}

impl Widget for &CornerRing {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        for (x, y, glyph) in corner_positions(area) {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_char(glyph).set_fg(self.color);
            }
        }
    }
}

#[must_use]
pub(crate) fn corner_positions(area: Rect) -> [(u16, u16, char); 4] {
    let glyphs = CornerGlyphs::default();
    let right = area.x + area.width.saturating_sub(1);
    let bottom = area.y + area.height.saturating_sub(1);
    [
        (area.x, area.y, glyphs.top_left),
        (right, area.y, glyphs.top_right),
        (area.x, bottom, glyphs.bottom_left),
        (right, bottom, glyphs.bottom_right),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::Rect,
        style::Color,
        widgets::Widget,
    };

    use crate::primitive::{
        corner_brackets::{CornerRing, corner_positions, expand},
        glyphs::CornerGlyphs,
    };

    fn widget_snapshot<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(widget, frame.area()))
            .unwrap();
        format!("{}", terminal.backend())
    }

    fn symbols(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                buffer.cell((x, y)).map(|cell| cell.symbol().to_string())
            })
            .collect()
    }

    #[test]
    fn corner_ring_paints_only_the_four_corners() {
        let area = Rect::new(0, 0, 5, 3);
        let mut buffer = Buffer::empty(area);
        CornerRing {
            color: Color::White,
        }
        .render(area, &mut buffer);

        let expected = Buffer::with_lines(["⌜   ⌝", "     ", "⌞   ⌟"]);
        assert_eq!(symbols(&buffer), symbols(&expected));
        assert_eq!(
            buffer.cell((0, 0)).and_then(|cell| cell.style().fg),
            Some(Color::White)
        );
    }

    #[test]
    fn corner_ring_off_buffer_drops_the_corners_that_fall_outside() {
        let buffer_area = Rect::new(0, 0, 4, 2);
        let mut buffer = Buffer::empty(buffer_area);
        CornerRing {
            color: Color::White,
        }
        .render(Rect::new(2, 1, 5, 4), &mut buffer);

        assert_eq!(symbols(&buffer), "      ⌜ ");
    }

    #[test]
    fn corner_ring_widget_snapshot() {
        let rendered = widget_snapshot(
            &CornerRing {
                color: Color::White,
            },
            8,
            4,
        );
        insta::assert_snapshot!("corner_ring_8x4", rendered);
    }

    #[test]
    fn expand_grows_the_rect_by_margin_on_every_side() {
        let area = Rect::new(5, 5, 10, 4);
        assert_eq!(expand(area, 1), Rect::new(4, 4, 12, 6));
        assert_eq!(expand(area, 0), area);
    }

    #[test]
    fn expand_saturates_instead_of_underflowing_at_the_origin() {
        let area = Rect::new(0, 0, 3, 3);
        assert_eq!(expand(area, 1), Rect::new(0, 0, 5, 5));
    }

    #[test]
    fn positions_land_on_the_four_corners() {
        let area = Rect::new(2, 3, 10, 4);
        let glyphs = CornerGlyphs::default();
        assert_eq!(
            corner_positions(area),
            [
                (2, 3, glyphs.top_left),
                (11, 3, glyphs.top_right),
                (2, 6, glyphs.bottom_left),
                (11, 6, glyphs.bottom_right),
            ]
        );
    }

    #[test]
    fn single_cell_area_collapses_all_corners_to_it() {
        let area = Rect::new(5, 5, 1, 1);
        let glyphs = CornerGlyphs::default();
        for (x, y, _) in corner_positions(area) {
            assert_eq!((x, y), (5, 5));
        }
        assert_eq!(corner_positions(area)[3].2, glyphs.bottom_right);
    }

    #[test]
    fn zero_size_area_still_returns_four_positions_at_its_origin() {
        let area = Rect::new(7, 7, 0, 0);
        for (x, y, _) in corner_positions(area) {
            assert_eq!((x, y), (7, 7));
        }
    }
}
