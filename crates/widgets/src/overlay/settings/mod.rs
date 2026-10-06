mod rows;
pub(crate) mod view;

use kernel::domain::{
    appearance_rows::appearance_rows,
    geometry::Cells,
    index::RowIndex,
    setting_row::SettingRow,
};
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
            OverlayAreas,
            column_width,
            leading_cells,
        },
        settings::{
            rows::{SettingsColumns, SettingsTableRow, settings_row},
            view::{max_value_width, settings_label},
        },
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        glyphs::{self, TITLE_SEPARATOR},
        list_chrome::{ScrollAreas, scroll_offset},
        text::truncate_from_left,
    },
    theme::active_theme::ActiveTheme,
};

const LABEL_GAP: Cells = Cells(2);

#[derive(Debug)]
pub(crate) struct SettingsWidget<'a> {
    theme: ActiveTheme<'a>,
    view: SettingsView<'a>,
    current: SettingRow,
    avoid: &'a [Rect],
}

impl<'a> SettingsWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        view: SettingsView<'a>,
        current: SettingRow,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            view,
            current,
            avoid: &[],
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }
}

struct SettingsContent {
    rows: Vec<SettingRow>,
    title: String,
    width: Cells,
}

impl SettingsWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.content()).areas(screen))
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::List(areas) = areas else {
            return;
        };
        let Canvas { area, buffer } = canvas;
        let content = self.content();
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
        let (table, mut table_rows) = self.rows_table(&areas, &content.rows);
        StatefulWidget::render(table, areas.rows, buffer, &mut table_rows);
    }

    fn content(&self) -> SettingsContent {
        let rows = SettingRow::all(&appearance_rows(self.view.appearance));
        let width = settings_content_width(&rows, &self.view);
        SettingsContent {
            title: modal_title_text(&self.view.music_dir_label(), width),
            rows,
            width,
        }
    }

    fn placement<'content>(
        &self,
        content: &'content SettingsContent,
    ) -> ModalPlacement<'content>
    where
        Self: 'content,
    {
        ModalPlacement {
            container: ModalContainer::Modal(self.avoid),
            border_title: Line::default(),
            modal_title: &content.title,
            content_width: content.width,
            content_rows: Cells(small_count_u16(content.rows.len())),
            hint: None,
            theme: self.theme,
        }
    }

    fn rows_table(
        &self,
        areas: &ScrollAreas,
        rows: &[SettingRow],
    ) -> (Table<'_>, TableState) {
        let inner = areas.rows;
        let colors = self.theme.colors();
        let label_width = label_column_width(rows);
        let selected = rows
            .iter()
            .position(|row| *row == self.current)
            .map(RowIndex::new);
        let columns = SettingsColumns::for_width(
            column_width(areas),
            leading_cells(areas),
            label_width,
        );
        let offset = selected.map_or(0, |at| {
            scroll_offset(at.get(), rows.len(), usize::from(inner.height))
        });
        let table = Table::new(
            rows.iter().map(|&row| {
                settings_row(
                    &SettingsTableRow {
                        row,
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
        let table_rows = TableState::new()
            .with_offset(offset)
            .with_selected(selected.map(RowIndex::get));
        (table, table_rows)
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
    let path = truncate_from_left(music_dir, path_budget);
    format!("{}{TITLE_SEPARATOR}{path}", glyphs::settings::TITLE_WORD)
}

fn label_column_width(rows: &[SettingRow]) -> Cells {
    let widest = rows
        .iter()
        .map(|&row| settings_label(row).width())
        .max()
        .unwrap_or(0);
    cells(widest.saturating_add(LABEL_GAP.count()))
}

fn settings_content_width(rows: &[SettingRow], view: &SettingsView<'_>) -> Cells {
    let label_width = label_column_width(rows);
    let widest_value = rows
        .iter()
        .map(|&row| max_value_width(row, view))
        .max()
        .unwrap_or(0);
    cells(label_width.count().saturating_add(widest_value))
}

fn cells(width: usize) -> Cells {
    Cells(small_count_u16(width))
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    use kernel::domain::{
        appearance::AppearanceSettings,
        crossfade::Crossfade,
        setting_row::AppearanceSetting,
        settings::ReplayGain,
    };

    use crate::overlay::settings::view::SettingsView;

    pub(crate) fn appearance_rows() -> Vec<AppearanceSetting> {
        kernel::domain::appearance_rows::appearance_rows(AppearanceSettings::default())
    }

    pub(crate) fn settings_values() -> SettingsView<'static> {
        SettingsView {
            crossfade: Crossfade::default(),
            replay_gain: ReplayGain::On,
            theme: "noir",
            themes: &[],
            sleep_presets: &[],
            music_dir: Path::new("/home/user/Music"),
            home: None,
            output_device: None,
            output_devices: &[],
            appearance: AppearanceSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::domain::{
        appearance::CoverMode,
        setting_row::SettingRow,
        theme::ThemeName,
    };
    use ratatui::layout::Rect;

    use crate::{
        overlay::{
            modal::placement::OverlayAreas,
            settings::{
                SettingsWidget,
                test_support::{appearance_rows, settings_values},
            },
        },
        primitive::canvas::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn outer_rect(widget: &SettingsWidget<'_>, screen: Rect) -> Option<Rect> {
        match widget.areas(screen) {
            OverlayAreas::List(areas) => Some(areas.outer),
            OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => None,
        }
    }

    #[test]
    fn settings_overlay_lists_every_row_with_its_label_and_value() {
        let theme = noir();
        let custom = appearance_rows();
        let widget = SettingsWidget::new(
            settings_values(),
            SettingRow::first(&custom),
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
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let widget =
            SettingsWidget::new(settings_values(), SettingRow::Crossfade, active);
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();
        let selection_background = active.colors().selection_background;
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
    fn settings_overlay_shows_the_current_theme_and_a_custom_appearance_row() {
        let theme = noir();
        let custom = appearance_rows();
        let widget = SettingsWidget::new(
            settings_values(),
            SettingRow::first(&custom),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "noir").is_some());
        assert!(find_text(&buffer, "Cover mode").is_some());
    }

    #[test]
    fn settings_title_keeps_the_path_tail_visible_at_a_narrow_width() {
        let theme = noir();
        let custom = appearance_rows();
        let mut with_long_path = settings_values();
        with_long_path.music_dir =
            Path::new("/Users/testuser/Music/Library/Deeply/Nested/Folder/apple-music");
        let widget = SettingsWidget::new(
            with_long_path,
            SettingRow::first(&custom),
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
        let custom = appearance_rows();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let themes = [
            ThemeName::from_static("noir"),
            ThemeName::from_static("gruvbox-light"),
        ];
        let screen = Rect::new(0, 0, 80, 28);

        let mut with_noir = settings_values();
        with_noir.themes = &themes;
        with_noir.theme = "noir";
        let noir_widget =
            SettingsWidget::new(with_noir, SettingRow::first(&custom), active);

        let mut with_gruvbox = settings_values();
        with_gruvbox.themes = &themes;
        with_gruvbox.theme = "gruvbox-light";
        with_gruvbox.appearance.cover_mode = CoverMode::Off;
        let gruvbox_widget =
            SettingsWidget::new(with_gruvbox, SettingRow::first(&custom), active);

        let outer_noir = outer_rect(&noir_widget, screen);
        let outer_gruvbox = outer_rect(&gruvbox_widget, screen);
        assert!(outer_noir.is_some());
        assert_eq!(outer_noir, outer_gruvbox);
    }

    #[test]
    fn settings_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let custom = appearance_rows();
        let widget = SettingsWidget::new(
            settings_values(),
            SettingRow::first(&custom),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let frame = rendered(4, 3, |frame| frame.render_widget(&widget, frame.area()))
            .to_string();
        assert_eq!(frame.lines().count(), 3);
    }
}
