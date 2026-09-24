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
            columns::{HelpColors, HelpColumn, HelpColumnFit, select_help_columns},
            groups::{HelpLayout, build_help_groups, small_count_u16},
        },
        modal::{ModalChrome, ModalPlacement, OverlayAreas, OverlayContainer},
    },
    primitive::{canvas::Canvas, glyphs::HelpGlyphs, inset::Inset},
    theme::ActiveTheme,
};

struct HelpColumnLayout {
    body: Rect,
    columns: Vec<HelpColumn>,
    column_gap: u16,
}

fn render_help_columns(input: &HelpColumnLayout, buffer: &mut Buffer) {
    let body = input.body;
    let widths: Vec<Constraint> = input
        .columns
        .iter()
        .map(|column| Constraint::Length(column.width.min(body.width)))
        .collect();
    let rects = Layout::horizontal(widths)
        .spacing(input.column_gap)
        .flex(Flex::Center)
        .split(body);
    let chord_gap = HelpLayout::default().chord_gap;

    for (&rect, column) in rects.iter().zip(&input.columns) {
        let constraints = column.constraints();
        Table::new(column.rows.clone(), constraints)
            .column_spacing(chord_gap)
            .render(rect, buffer);
    }
}

const TITLE: &str = "KEYS";

#[derive(Debug)]
pub struct HelpOverlay<'a> {
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
    pub fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.content(screen)).areas(screen))
    }

    pub fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
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
        render_help_columns(
            &HelpColumnLayout {
                body: areas.content,
                columns: content.columns,
                column_gap: content.column_gap,
            },
            buffer,
        );
    }

    fn colors(&self) -> HelpColors {
        let theme = self.theme;
        HelpColors {
            title: theme.frame(),
            key: theme.muted_accent(),
            description: theme.text(),
        }
    }

    fn content(&self, screen: Rect) -> HelpContent {
        let layout = HelpLayout::default();
        let groups = build_help_groups(self.bindings);
        let columns = select_help_columns(&HelpColumnFit {
            groups: &groups,
            colors: self.colors(),
            layout,
            glyphs: HelpGlyphs::default(),
            full: screen,
        });
        let column_gap = if columns.len() > 1 {
            layout.column_gap
        } else {
            0
        };
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
            inset: Inset::default(),
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
            chrome: ModalChrome::default(),
        }
    }
}

impl Widget for &HelpOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::KeymapOverrides,
        update::keymap::{Bindings, KeyBinding},
    };
    use rstest::rstest;

    use crate::{
        overlay::help::HelpOverlay,
        scene::fixtures::{noir, painted},
        theme::{ActiveTheme, ColorDepth},
    };

    fn bindings() -> Vec<KeyBinding> {
        Bindings::new(&KeymapOverrides::default())
            .as_slice()
            .to_vec()
    }

    fn rendered(width: u16, height: u16) -> String {
        let theme = noir();
        let bindings = bindings();
        let overlay = HelpOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            bindings: &bindings,
            avoid: &[],
        };
        painted(&overlay, width, height)
    }

    #[rstest]
    #[case::wide(120, 40)]
    #[case::narrow(50, 16)]
    fn help_overlay_layout_by_terminal_size(#[case] width: u16, #[case] height: u16) {
        insta::with_settings!({ snapshot_suffix => format!("{width}x{height}") }, {
            insta::assert_snapshot!(rendered(width, height));
        });
    }

    #[test]
    fn help_overlay_does_not_panic_on_a_tiny_terminal() {
        let _ = rendered(4, 3);
    }
}
