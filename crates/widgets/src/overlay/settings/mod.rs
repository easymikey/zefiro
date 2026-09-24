mod rows;
mod values;

use kernel::domain::SettingRow;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{StatefulWidget, Table, TableState, Widget},
};
use unicode_width::UnicodeWidthStr;
pub(crate) use values::SettingsView;

use crate::{
    overlay::{
        modal::{
            ModalChrome,
            ModalPlacement,
            ModalRowColors,
            ModalScrollAreas,
            OverlayAreas,
            OverlayContainer,
            column_width,
            lead_cells,
        },
        settings::{
            rows::{SettingsColumns, SettingsRowView, settings_row},
            values::{max_value_width, settings_label},
        },
    },
    primitive::{
        canvas::Canvas,
        glyphs::{SettingsGlyphs, TITLE_SEPARATOR},
        inset::Inset,
        list_chrome::scroll_offset,
        text::truncate_from_left,
    },
    theme::ActiveTheme,
};

const LABEL_GAP: usize = 2;

#[derive(Debug)]
pub struct SettingsOverlay<'a> {
    pub theme: ActiveTheme<'a>,
    pub values: SettingsView<'a>,
    pub selected: usize,
    pub avoid: &'a [Rect],
}

impl<'a> SettingsOverlay<'a> {
    #[must_use]
    pub fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.modal_title()).areas(screen))
    }

    pub fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::List(areas) = areas else {
            return;
        };
        let Canvas { area, buffer } = canvas;
        self.placement(&self.modal_title()).paint(
            areas,
            Canvas {
                area,
                buffer: &mut *buffer,
            },
        );
        if areas.content.width == 0 || areas.content.height == 0 {
            return;
        }
        self.render_rows(&areas, buffer);
    }

    fn rows(&self) -> Vec<SettingRow> {
        SettingRow::all(self.values.custom_rows)
    }

    fn modal_title(&self) -> String {
        modal_title_text(self.values.music_dir, self.content_width())
    }

    fn content_width(&self) -> u16 {
        settings_content_width(&self.rows(), &self.values)
    }

    fn placement<'title>(&self, modal_title: &'title str) -> ModalPlacement<'title>
    where
        'a: 'title,
    {
        ModalPlacement {
            inset: Inset::default(),
            container: OverlayContainer::Modal { avoid: self.avoid },
            border_title: Line::default(),
            modal_title,
            content_width: self.content_width(),
            content_rows: u16::try_from(self.rows().len()).unwrap_or(u16::MAX),
            hint: None,
            theme: self.theme,
            chrome: ModalChrome::default(),
        }
    }

    fn render_rows(&self, areas: &ModalScrollAreas, buffer: &mut Buffer) {
        let inner = areas.rows;
        let colors = ModalRowColors::from_theme(&self.theme);
        let rows = self.rows();
        let label_width = label_column_width(&rows);
        let columns = SettingsColumns::resolve(
            column_width(areas),
            lead_cells(areas),
            label_width,
        );
        let offset =
            scroll_offset(self.selected, rows.len(), usize::from(inner.height));
        let table = Table::new(
            rows.iter().map(|&row| {
                settings_row(
                    &SettingsRowView {
                        row,
                        values: &self.values,
                        columns,
                    },
                    colors,
                )
            }),
            columns.constraints(),
        )
        .column_spacing(0)
        .row_highlight_style(colors.highlight());
        let mut table_rows = TableState::new()
            .with_offset(offset)
            .with_selected(Some(self.selected));
        StatefulWidget::render(table, inner, buffer, &mut table_rows);
    }
}

impl Widget for &SettingsOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

fn modal_title_text(music_dir: &str, content_width: u16) -> String {
    let glyphs = SettingsGlyphs::default();
    let fixed_width = glyphs.title_word.width() + TITLE_SEPARATOR.width();
    let path_budget = usize::from(content_width).saturating_sub(fixed_width);
    let path = truncate_from_left(music_dir, path_budget);
    format!("{}{TITLE_SEPARATOR}{path}", glyphs.title_word)
}

fn label_column_width(rows: &[SettingRow]) -> usize {
    let widest = rows
        .iter()
        .map(|&row| settings_label(row).width())
        .max()
        .unwrap_or(0);
    widest + LABEL_GAP
}

fn settings_content_width(rows: &[SettingRow], values: &SettingsView<'_>) -> u16 {
    let label_width = label_column_width(rows);
    let widest_value = rows
        .iter()
        .map(|&row| max_value_width(row, values))
        .max()
        .unwrap_or(0);
    u16::try_from(label_width + widest_value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use config::CoverStyle;
    use ratatui::layout::Rect;

    use crate::{
        overlay::{modal::OverlayAreas, settings::SettingsOverlay},
        scene::fixtures::{
            custom_rows,
            find_text,
            noir,
            painted,
            painted_buffer,
            settings_values,
        },
        theme::{ActiveTheme, ColorDepth},
    };

    fn outer_rect(overlay: &SettingsOverlay<'_>, screen: Rect) -> Option<Rect> {
        match overlay.areas(screen) {
            OverlayAreas::List(areas) => Some(areas.outer),
            OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => None,
        }
    }

    #[test]
    fn settings_overlay_lists_every_row_with_its_label_and_value() {
        let theme = noir();
        let custom = custom_rows();
        let overlay = SettingsOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn settings_overlay_highlights_the_selected_row() {
        let theme = noir();
        let custom = custom_rows();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let overlay = SettingsOverlay {
            theme: active,
            values: settings_values(&custom),
            selected: 1 + custom.len(),
            avoid: &[],
        };
        let buffer = painted_buffer(&overlay, 80, 28);
        let selection_bg = active.selection_bg();
        let (theme_x, theme_y) = find_text(&buffer, "Theme").unwrap();
        let (crossfade_x, crossfade_y) = find_text(&buffer, "Crossfade").unwrap();
        assert_eq!(
            buffer[(crossfade_x, crossfade_y)].style().bg,
            Some(selection_bg)
        );
        assert_ne!(buffer[(theme_x, theme_y)].style().bg, Some(selection_bg));
    }

    #[test]
    fn settings_overlay_shows_the_current_theme_and_a_custom_appearance_row() {
        let theme = noir();
        let custom = custom_rows();
        let overlay = SettingsOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        let buffer = painted_buffer(&overlay, 80, 28);
        assert!(find_text(&buffer, "noir").is_some());
        assert!(find_text(&buffer, "Cover style").is_some());
    }

    #[test]
    fn settings_title_keeps_the_path_tail_visible_at_a_narrow_width() {
        let theme = noir();
        let custom = custom_rows();
        let mut with_long_path = settings_values(&custom);
        with_long_path.music_dir =
            "/Users/testuser/Music/Library/Deeply/Nested/Folder/apple-music";
        let overlay = SettingsOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: with_long_path,
            selected: 0,
            avoid: &[],
        };
        let buffer = painted_buffer(&overlay, 60, 19);
        assert!(find_text(&buffer, "apple-music").is_some());
    }

    #[test]
    fn the_modal_outer_rect_stays_put_across_a_theme_and_an_appearance_change() {
        let theme = noir();
        let custom = custom_rows();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let themes = ["noir".to_string(), "gruvbox-light".to_string()];
        let screen = Rect::new(0, 0, 80, 28);

        let mut with_noir = settings_values(&custom);
        with_noir.themes = &themes;
        with_noir.theme = "noir";
        let overlay_noir = SettingsOverlay {
            theme: active,
            values: with_noir,
            selected: 0,
            avoid: &[],
        };

        let mut with_gruvbox = settings_values(&custom);
        with_gruvbox.themes = &themes;
        with_gruvbox.theme = "gruvbox-light";
        with_gruvbox.appearance.cover_style = CoverStyle::Off;
        let overlay_gruvbox = SettingsOverlay {
            theme: active,
            values: with_gruvbox,
            selected: 0,
            avoid: &[],
        };

        let outer_noir = outer_rect(&overlay_noir, screen);
        let outer_gruvbox = outer_rect(&overlay_gruvbox, screen);
        assert!(outer_noir.is_some());
        assert_eq!(outer_noir, outer_gruvbox);
    }

    #[test]
    fn settings_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let custom = custom_rows();
        let overlay = SettingsOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        let _ = painted(&overlay, 4, 3);
    }
}
