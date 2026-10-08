mod columns;
pub(crate) mod groups;

use kernel::domain::geometry::Cells;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    text::Line,
    widgets::{Table, Widget},
};

use crate::{
    overlay::{
        help::{
            columns::{HelpColumn, columns_width, select_help_columns},
            groups::{CHORD_GAP, COLUMN_GAP, HelpGroups},
        },
        modal::placement::{ModalContainer, ModalPlacement},
    },
    primitive::{canvas::Canvas, list_chrome::ScrollAreas},
    theme::active_theme::ActiveTheme,
};

fn paint_help_columns(
    help_columns: &HelpColumns,
    theme: &ActiveTheme<'_>,
    canvas: Canvas<'_>,
) {
    let Canvas { area: body, buffer } = canvas;
    let widths: Vec<Constraint> = help_columns
        .columns
        .iter()
        .map(|column| Constraint::Length(column.width.0.min(body.width)))
        .collect();
    let rects = Layout::horizontal(widths)
        .spacing(help_columns.column_gap_width.0)
        .flex(Flex::Center)
        .split(body);
    let chord_gap = CHORD_GAP;

    for (&rect, column) in rects.iter().zip(&help_columns.columns) {
        let constraints = column.constraints();
        Table::new(column.rows(theme), constraints)
            .column_spacing(chord_gap)
            .render(rect, buffer);
    }
}

const TITLE: &str = "KEYS";

#[derive(Debug)]
pub(crate) struct HelpWidget<'a> {
    theme: ActiveTheme<'a>,
    help_columns: &'a HelpColumns,
    avoid: &'a [Rect],
}

#[derive(Debug, Clone, PartialEq)]
pub struct HelpColumns {
    columns: Vec<HelpColumn>,
    column_gap_width: Cells,
}

impl HelpColumns {
    #[must_use]
    pub(crate) fn new(groups: &HelpGroups, screen: Rect) -> Self {
        let columns = select_help_columns(groups, screen);
        let column_gap_width = if columns.len() > 1 {
            Cells(COLUMN_GAP)
        } else {
            Cells(0)
        };
        Self {
            columns,
            column_gap_width,
        }
    }
}

impl<'a> HelpWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        help_columns: &'a HelpColumns,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            help_columns,
            avoid: &[],
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ScrollAreas {
        self.placement().areas(screen)
    }

    pub(crate) fn paint(&self, areas: ScrollAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        self.placement().paint(
            areas,
            Canvas {
                area,
                buffer: &mut *buffer,
            },
        );
        if areas.content.width == 0 || areas.content.height == 0 {
            return;
        }
        paint_help_columns(
            self.help_columns,
            &self.theme,
            Canvas {
                area: areas.content,
                buffer,
            },
        );
    }

    fn placement(&self) -> ModalPlacement<'a> {
        let columns = &self.help_columns.columns;
        ModalPlacement {
            container: ModalContainer::Floating(self.avoid),
            border_title: Line::default(),
            modal_title: TITLE,
            content_width: columns_width(columns, self.help_columns.column_gap_width),
            content_rows: columns
                .iter()
                .map(|column| column.height)
                .fold(Cells(0), Cells::max),
            theme: self.theme,
        }
    }
}

impl Widget for &HelpWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::update::keymap::bindings::Keymap;
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::help::{HelpColumns, HelpWidget, groups::HelpGroups},
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn help_frame(width: u16, height: u16) -> String {
        let theme = noir();
        let keymap = Keymap::default();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let groups = HelpGroups::new(keymap.bindings());
        let help_columns = HelpColumns::new(&groups, Rect::new(0, 0, width, height));
        let overlay_widget = HelpWidget::new(&help_columns, active_theme);
        rendered(width, height, |frame| {
            frame.render_widget(&overlay_widget, frame.area());
        })
        .to_string()
    }

    #[rstest]
    #[case::wide(120, 40)]
    #[case::narrow(50, 16)]
    fn help_overlay_fits_its_columns_to_the_terminal(
        #[case] width: u16,
        #[case] height: u16,
    ) {
        insta::with_settings!({ snapshot_suffix => format!("{width}x{height}") }, {
            insta::assert_snapshot!(help_frame(width, height));
        });
    }

    #[test]
    fn help_overlay_does_not_panic_on_a_tiny_terminal() {
        assert_eq!(help_frame(4, 3).lines().count(), 3);
    }
}
