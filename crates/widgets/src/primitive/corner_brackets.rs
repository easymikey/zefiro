use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};

use crate::primitive::glyphs;

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
pub(crate) struct CornerBracketsWidget {
    color: Color,
}

impl CornerBracketsWidget {
    #[must_use]
    pub(crate) fn new(color: Color) -> Self {
        Self { color }
    }
}

impl Widget for &CornerBracketsWidget {
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
    let right = area.x + area.width.saturating_sub(1);
    let bottom = area.y + area.height.saturating_sub(1);
    [
        (area.x, area.y, glyphs::corner::TOP_LEFT),
        (right, area.y, glyphs::corner::TOP_RIGHT),
        (area.x, bottom, glyphs::corner::BOTTOM_LEFT),
        (right, bottom, glyphs::corner::BOTTOM_RIGHT),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};
    use rstest::rstest;

    use crate::primitive::{
        corner_brackets::{CornerBracketsWidget, corner_positions, expand},
        glyphs::corner::{BOTTOM_LEFT, BOTTOM_RIGHT, TOP_LEFT, TOP_RIGHT},
    };

    fn symbols(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                buffer.cell((x, y)).map(|cell| cell.symbol().to_string())
            })
            .collect()
    }

    #[test]
    fn corner_brackets_paints_only_the_four_corners() {
        let area = Rect::new(0, 0, 5, 3);
        let mut buffer = Buffer::empty(area);
        CornerBracketsWidget::new(Color::White).render(area, &mut buffer);

        let expected = Buffer::with_lines(["⌜   ⌝", "     ", "⌞   ⌟"]);
        assert_eq!(symbols(&buffer), symbols(&expected));
        assert_eq!(
            buffer.cell((0, 0)).and_then(|cell| cell.style().fg),
            Some(Color::White)
        );
    }

    #[test]
    fn corner_brackets_off_buffer_drops_the_corners_that_fall_outside() {
        let buffer_area = Rect::new(0, 0, 4, 2);
        let mut buffer = Buffer::empty(buffer_area);
        CornerBracketsWidget::new(Color::White)
            .render(Rect::new(2, 1, 5, 4), &mut buffer);

        assert_eq!(symbols(&buffer), "      ⌜ ");
    }

    #[rstest]
    #[case::one_cell_margin(Rect::new(5, 5, 10, 4), 1, Rect::new(4, 4, 12, 6))]
    #[case::no_margin(Rect::new(5, 5, 10, 4), 0, Rect::new(5, 5, 10, 4))]
    #[case::saturates_at_the_origin(Rect::new(0, 0, 3, 3), 1, Rect::new(0, 0, 5, 5))]
    fn expand_grows_the_rect_by_margin_on_every_side(
        #[case] area: Rect,
        #[case] margin: u16,
        #[case] expected: Rect,
    ) {
        assert_eq!(expand(area, margin), expected);
    }

    #[rstest]
    #[case::a_wide_area(
        Rect::new(2, 3, 10, 4),
        [(2, 3, TOP_LEFT), (11, 3, TOP_RIGHT), (2, 6, BOTTOM_LEFT), (11, 6, BOTTOM_RIGHT)]
    )]
    #[case::a_single_cell(
        Rect::new(5, 5, 1, 1),
        [(5, 5, TOP_LEFT), (5, 5, TOP_RIGHT), (5, 5, BOTTOM_LEFT), (5, 5, BOTTOM_RIGHT)]
    )]
    #[case::a_zero_size_area(
        Rect::new(7, 7, 0, 0),
        [(7, 7, TOP_LEFT), (7, 7, TOP_RIGHT), (7, 7, BOTTOM_LEFT), (7, 7, BOTTOM_RIGHT)]
    )]
    fn positions_land_on_the_four_corners(
        #[case] area: Rect,
        #[case] expected: [(u16, u16, char); 4],
    ) {
        assert_eq!(corner_positions(area), expected);
    }
}
