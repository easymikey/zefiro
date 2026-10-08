use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::Style,
    widgets::{Paragraph, Widget},
};

use crate::{
    primitive::{
        span::{line, text},
        truncate::{truncate, truncate_owned},
    },
    theme::active_theme::ActiveTheme,
};

const HEADLINE: &str = "Terminal too small.";
const RESIZE_PREFIX: &str = "Resize to at least ";
const DIMENSION_SEPARATOR: &str = "\u{00d7}";
const CURRENT_OPEN: &str = "(now ";
const CURRENT_CLOSE: &str = ")";

#[derive(Debug, Clone, Copy)]
pub(crate) struct TooSmallWidget<'a> {
    minimum: Size,
    theme: ActiveTheme<'a>,
}

impl<'a> TooSmallWidget<'a> {
    #[must_use]
    pub(crate) fn new(minimum: Size, active_theme: ActiveTheme<'a>) -> Self {
        Self {
            minimum,
            theme: active_theme,
        }
    }
}

impl Widget for &TooSmallWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let colors = self.theme.colors();
        let text_style = Style::default().fg(colors.foreground);
        let dim_style = Style::default().fg(colors.muted_foreground);
        let width = usize::from(area.width);
        let fit = |line: String| truncate_owned(line, width);
        let resize_line = fit(format!(
            "{}{}{}{}",
            RESIZE_PREFIX, self.minimum.width, DIMENSION_SEPARATOR, self.minimum.height
        ));
        let current_line = fit(format!(
            "{}{}{}{}{}",
            CURRENT_OPEN, area.width, DIMENSION_SEPARATOR, area.height, CURRENT_CLOSE
        ));
        let lines = vec![
            line([text(truncate(HEADLINE, width)).style(text_style)])
                .alignment(Alignment::Center),
            line([text(resize_line).style(text_style)]).alignment(Alignment::Center),
            line([text(current_line).style(dim_style)]).alignment(Alignment::Center),
        ];
        let content_height = u16::try_from(lines.len()).unwrap_or(0).min(area.height);
        let target = Rect {
            y: area.y + area.height.saturating_sub(content_height) / 2,
            height: content_height,
            ..area
        };
        Paragraph::new(lines).render(target, buffer);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        buffer::Buffer,
        layout::{Rect, Size},
        widgets::Widget,
    };
    use rstest::rstest;

    use crate::{
        screen::too_small::TooSmallWidget,
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[rstest]
    #[case::centred_in_a_tall_area(Rect::new(0, 3, 30, 7), [5, 6, 7])]
    #[case::at_the_top_of_a_short_area(Rect::new(0, 1, 30, 4), [1, 2, 3])]
    fn the_three_lines_sit_in_the_vertical_middle_of_the_area(
        #[case] area: Rect,
        #[case] rows: [u16; 3],
    ) {
        let theme = noir();
        let widget = TooSmallWidget::new(
            Size::new(80, 24),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 12));
        (&widget).render(area, &mut buffer);
        let row_of = |needle: &str| {
            (0..buffer.area.height).find(|&y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .contains(needle)
            })
        };
        assert_eq!(
            [
                row_of("Terminal too small."),
                row_of("Resize to at least 80"),
                row_of("(now 30"),
            ],
            rows.map(Some)
        );
    }
}
