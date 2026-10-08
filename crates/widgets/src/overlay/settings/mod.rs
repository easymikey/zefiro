mod rows;
pub(crate) mod view;

use kernel::domain::{geometry::Cells, index::RowIndex, setting_row::SettingRow};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{StatefulWidget, Table, TableState, Widget},
};
use unicode_width::UnicodeWidthStr;
use view::SettingsView;

use crate::{
    overlay::{
        modal::placement::{
            ModalContainer,
            ModalPlacement,
            column_width,
            leading_cells,
        },
        settings::{
            rows::{SettingsColumns, SettingsTableRow, settings_row},
            view::{settings_label, widest_value},
        },
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        glyphs::{self, TITLE_SEPARATOR},
        list_chrome::{ScrollAreas, scroll_offset},
        truncate::truncate_head,
    },
    theme::active_theme::ActiveTheme,
};

const LABEL_GAP: Cells = Cells(2);

#[derive(Debug)]
pub(crate) struct SettingsWidget<'a> {
    theme: ActiveTheme<'a>,
    view: SettingsView<'a>,
    settings_table: &'a SettingsTable,
    selected: SettingRow,
    avoid: &'a [Rect],
}

impl<'a> SettingsWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        view: SettingsView<'a>,
        settings_table: &'a SettingsTable,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            view,
            settings_table,
            selected: SettingRow::first(),
            avoid: &[],
        }
    }

    #[must_use]
    pub(crate) fn selected(mut self, selected: SettingRow) -> Self {
        self.selected = selected;
        self
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTable {
    title: String,
    width: Cells,
    label_width: Cells,
}

impl SettingsTable {
    #[must_use]
    pub(crate) fn new(view: &SettingsView<'_>) -> Self {
        let label_width = label_column_width();
        let width = settings_content_width(label_width, view);
        Self {
            title: modal_title_text(&view.music_dir_label(), width),
            width,
            label_width,
        }
    }
}

impl<'a> SettingsWidget<'a> {
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
        let (table, mut table_state) = self.rows_table(&areas);
        StatefulWidget::render(table, areas.rows, buffer, &mut table_state);
    }

    fn placement(&self) -> ModalPlacement<'a> {
        let settings_table = self.settings_table;
        ModalPlacement {
            container: ModalContainer::Floating(self.avoid),
            border_title: Line::default(),
            modal_title: &settings_table.title,
            content_width: settings_table.width,
            content_rows: Cells(small_count_u16(SettingRow::ALL.len())),
            theme: self.theme,
        }
    }

    fn rows_table(&self, areas: &ScrollAreas) -> (Table<'_>, TableState) {
        let rows = &SettingRow::ALL;
        let inner = areas.rows;
        let colors = self.theme.colors();
        let label_width = self.settings_table.label_width;
        let selected = RowIndex::new(self.selected.position());
        let columns = SettingsColumns::for_width(
            column_width(areas),
            leading_cells(areas),
            label_width,
        );
        let offset = scroll_offset(selected, rows.len(), usize::from(inner.height));
        let table = Table::new(
            rows.iter().map(|&row| {
                settings_row(
                    &SettingsTableRow {
                        setting_row: row,
                        view: &self.view,
                        columns,
                    },
                    colors,
                )
            }),
            columns.constraints(),
        )
        .column_spacing(0)
        .row_highlight_style(
            Style::default()
                .fg(colors.selection_foreground)
                .bg(colors.selection_background),
        );
        let table_state = TableState::new()
            .with_offset(offset)
            .with_selected(selected.get());
        (table, table_state)
    }
}

impl Widget for &SettingsWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

fn modal_title_text(music_dir: &str, content_width: Cells) -> String {
    let fixed_width = glyphs::settings::TITLE_WORD.width() + TITLE_SEPARATOR.width();
    let path_budget = content_width.count().saturating_sub(fixed_width);
    let path = truncate_head(music_dir, path_budget);
    format!("{}{TITLE_SEPARATOR}{path}", glyphs::settings::TITLE_WORD)
}

fn label_column_width() -> Cells {
    let widest = SettingRow::ALL
        .iter()
        .map(|&row| settings_label(row).width())
        .max()
        .unwrap_or(0);
    cells(widest.saturating_add(LABEL_GAP.count()))
}

fn settings_content_width(label_width: Cells, view: &SettingsView<'_>) -> Cells {
    let value_width = SettingRow::ALL
        .iter()
        .map(|&row| widest_value(row, view))
        .max()
        .unwrap_or(0);
    cells(label_width.count().saturating_add(value_width))
}

fn cells(width: usize) -> Cells {
    Cells(small_count_u16(width))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{path::Path, time::Duration};

    use kernel::domain::{
        appearance::{AppearanceSettings, CoverMode},
        crossfade::Crossfade,
        setting_row::SettingRow,
        settings::ReplayGain,
        theme::ThemeName,
    };
    use ratatui::layout::Rect;

    use crate::{
        overlay::settings::{SettingsTable, SettingsWidget, view::SettingsView},
        primitive::canvas::tests::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    pub(crate) fn settings_values() -> SettingsView<'static> {
        SettingsView {
            crossfade: Crossfade::default(),
            replay_gain: ReplayGain::On,
            theme: "noir",
            theme_names: &[],
            sleep_presets: &[],
            music_dir: Path::new("/home/user/Music"),
            home_dir: None,
            output_device_name: None,
            output_devices: &[],
            appearance_settings: AppearanceSettings::default(),
        }
    }

    #[test]
    fn settings_overlay_lists_every_row_with_its_label_and_value() {
        let theme = noir();
        let settings_table = SettingsTable::new(&settings_values());
        let widget = SettingsWidget::new(
            settings_values(),
            &settings_table,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn settings_overlay_highlights_the_selected_row() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let settings_table = SettingsTable::new(&settings_values());
        let widget =
            SettingsWidget::new(settings_values(), &settings_table, active_theme)
                .selected(SettingRow::Crossfade);
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();
        let selection_background = active_theme.colors().selection_background;
        let (theme_x, theme_y) = find_text(&buffer, "Theme").unwrap();
        let (crossfade_x, crossfade_y) = find_text(&buffer, "Crossfade").unwrap();
        assert_eq!(
            buffer[(crossfade_x, crossfade_y)].style().bg,
            Some(selection_background)
        );
        assert_ne!(
            buffer[(theme_x, theme_y)].style().bg,
            Some(selection_background)
        );
    }

    #[test]
    fn settings_title_keeps_the_path_tail_visible_at_a_narrow_width() {
        let theme = noir();
        let mut with_long_path = settings_values();
        with_long_path.music_dir =
            Path::new("/Users/testuser/Music/Library/Deeply/Nested/Folder/apple-music");
        let settings_table = SettingsTable::new(&with_long_path);
        let widget = SettingsWidget::new(
            with_long_path,
            &settings_table,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let buffer =
            rendered(60, 19, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "apple-music").is_some());
    }

    #[test]
    fn the_modal_outer_rect_stays_put_across_a_theme_and_an_appearance_change() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let themes = [
            ThemeName::from_static("noir"),
            ThemeName::from_static("gruvbox-light"),
        ];
        let screen = Rect::new(0, 0, 80, 28);

        let mut with_noir = settings_values();
        with_noir.theme_names = &themes;
        with_noir.theme = "noir";
        let noir_table = SettingsTable::new(&with_noir);
        let noir_widget = SettingsWidget::new(with_noir, &noir_table, active_theme);

        let mut with_gruvbox = settings_values();
        with_gruvbox.theme_names = &themes;
        with_gruvbox.theme = "gruvbox-light";
        with_gruvbox.appearance_settings.cover_mode = CoverMode::Off;
        let gruvbox_table = SettingsTable::new(&with_gruvbox);
        let gruvbox_widget =
            SettingsWidget::new(with_gruvbox, &gruvbox_table, active_theme);

        assert_eq!(
            noir_widget.areas(screen).outer,
            gruvbox_widget.areas(screen).outer
        );
    }

    #[test]
    fn a_custom_sleep_preset_list_fits_the_value_column() {
        let theme = noir();
        let presets =
            [100, 200, 300, 400, 720].map(|minutes| Duration::from_secs(minutes * 60));
        let mut with_presets = settings_values();
        with_presets.sleep_presets = &presets;
        let settings_table = SettingsTable::new(&with_presets);
        let widget = SettingsWidget::new(
            with_presets,
            &settings_table,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "100m, 200m, 300m, 400m, 720m").is_some());
    }
}
