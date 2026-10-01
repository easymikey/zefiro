mod columns;
mod groups;

use kernel::update::keymap::KeyBinding;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    text::Line,
    widgets::{Table, Widget},
};

use crate::{
    overlay::{
        help::{
            columns::{HelpColors, HelpColumn, select_help_columns},
            groups::{CHORD_GAP, COLUMN_GAP, build_help_groups, small_count_u16},
        },
        modal::{ModalPlacement, OverlayAreas, OverlayContainer},
    },
    primitive::{canvas::Canvas, inset::Inset},
    theme::{ActiveTheme, Role},
};

fn render_help_columns(body: Rect, content: &HelpContent, buffer: &mut Buffer) {
    let widths: Vec<Constraint> = content
        .columns
        .iter()
        .map(|column| Constraint::Length(column.width.min(body.width)))
        .collect();
    let rects = Layout::horizontal(widths)
        .spacing(content.column_gap)
        .flex(Flex::Center)
        .split(body);
    let chord_gap = CHORD_GAP;

    for (&rect, column) in rects.iter().zip(&content.columns) {
        let constraints = column.constraints();
        Table::new(column.rows.clone(), constraints)
            .column_spacing(chord_gap)
            .render(rect, buffer);
    }
}

const TITLE: &str = "KEYS";

#[derive(Debug)]
pub(crate) struct HelpOverlay<'a> {
    pub theme: ActiveTheme<'a>,
    pub bindings: &'a [KeyBinding],
    pub avoid: &'a [Rect],
}

struct HelpContent {
    columns: Vec<HelpColumn>,
    column_gap: u16,
}

impl<'a> HelpOverlay<'a> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.content(screen)).areas(screen))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
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
        render_help_columns(areas.content, &content, buffer);
    }

    fn colors(&self) -> HelpColors {
        let theme = self.theme;
        HelpColors {
            title: theme.role(Role::Frame),
            key: theme.muted_accent(),
            description: theme.role(Role::Text),
        }
    }

    fn content(&self, screen: Rect) -> HelpContent {
        let groups = build_help_groups(self.bindings);
        let columns = select_help_columns(&groups, self.colors(), screen);
        let column_gap = if columns.len() > 1 { COLUMN_GAP } else { 0 };
        HelpContent {
            columns,
            column_gap,
        }
    }

    fn placement<'content>(
        &self,
        content: &'content HelpContent,
    ) -> ModalPlacement<'content>
    where
        Self: 'content,
    {
        let columns = &content.columns;
        let gaps = small_count_u16(columns.len().saturating_sub(1));
        ModalPlacement {
            inset: Inset::overlay(),
            container: OverlayContainer::Modal { avoid: self.avoid },
            border_title: Line::default(),
            modal_title: TITLE,
            content_width: columns.iter().map(|column| column.width).sum::<u16>()
                + content.column_gap * gaps,
            content_rows: columns
                .iter()
                .map(|column| column.height)
                .fold(0u16, u16::max),
            hint: None,
            theme: self.theme,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        overlay::{help::HelpOverlay, rendered_canvas},
        test_support::{bindings, noir},
        theme::{ActiveTheme, ColorDepth},
    };

    fn help_frame(width: u16, height: u16) -> String {
        let theme = noir();
        let bindings = bindings();
        let overlay = HelpOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            bindings: &bindings,
            avoid: &[],
        };
        rendered_canvas(width, height, |canvas| {
            overlay.render_in(overlay.areas(canvas.area), canvas);
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
        let _ = help_frame(4, 3);
    }
}
