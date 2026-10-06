mod columns;
mod groups;

use kernel::{domain::geometry::Cells, update::keymap::chord::KeyBinding};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    text::Line,
    widgets::{Table, Widget},
};

use crate::{
    overlay::{
        help::{
            columns::{HelpColumn, select_help_columns},
            groups::{CHORD_GAP, COLUMN_GAP, HelpGroups},
        },
        modal::placement::{ModalContainer, ModalPlacement, OverlayAreas},
    },
    pixels::numeric::small_count_u16,
    primitive::canvas::Canvas,
    theme::active_theme::ActiveTheme,
};

fn paint_help_columns(body: Rect, help_columns: &HelpColumns, buffer: &mut Buffer) {
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
        Table::new(column.rows.clone(), constraints)
            .column_spacing(chord_gap)
            .render(rect, buffer);
    }
}

const TITLE: &str = "KEYS";

#[derive(Debug)]
pub(crate) struct HelpWidget<'a> {
    theme: ActiveTheme<'a>,
    bindings: &'a [KeyBinding],
    avoid: &'a [Rect],
}

struct HelpColumns {
    columns: Vec<HelpColumn>,
    column_gap_width: Cells,
}

impl<'a> HelpWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        bindings: &'a [KeyBinding],
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            bindings,
            avoid: &[],
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.content(screen)).areas(screen))
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::List(areas) = areas else {
            return;
        };
        let Canvas { area, buffer } = canvas;
        let content = self.content(area);
        self.placement(&content).paint(
            areas,
            Canvas {
                area,
                buffer: &mut *buffer,
            },
        );
        if areas.content.width == 0 || areas.content.height == 0 {
            return;
        }
        paint_help_columns(areas.content, &content, buffer);
    }

    fn content(&self, screen: Rect) -> HelpColumns {
        let groups = HelpGroups::new(self.bindings);
        let columns = select_help_columns(&groups, &self.theme, screen);
        let column_gap_width = if columns.len() > 1 {
            Cells(COLUMN_GAP)
        } else {
            Cells(0)
        };
        HelpColumns {
            columns,
            column_gap_width,
        }
    }

    fn placement<'content>(
        &self,
        help_columns: &'content HelpColumns,
    ) -> ModalPlacement<'content>
    where
        Self: 'content,
    {
        let columns = &help_columns.columns;
        let gaps = small_count_u16(columns.len().saturating_sub(1));
        ModalPlacement {
            container: ModalContainer::Floating(self.avoid),
            border_title: Line::default(),
            modal_title: TITLE,
            content_width: Cells(
                columns.iter().map(|column| column.width.0).sum::<u16>()
                    + help_columns.column_gap_width.0 * gaps,
            ),
            content_rows: columns
                .iter()
                .map(|column| column.height)
                .fold(Cells(0), Cells::max),
            hint: None,
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
    use rstest::rstest;

    use crate::{
        overlay::help::HelpWidget,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn help_frame(width: u16, height: u16) -> String {
        let theme = noir();
        let keymap = Keymap::default();
        let bindings = keymap.bindings();
        let overlay_widget =
            HelpWidget::new(bindings, ActiveTheme::new(&theme, ColorDepth::TrueColor));
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
