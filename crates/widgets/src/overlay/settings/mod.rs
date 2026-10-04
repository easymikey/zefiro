mod rows;
pub(crate) mod values;

use kernel::domain::{geometry::Cells, setting_row::SettingRow};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{StatefulWidget, Table, TableState, Widget},
};
use unicode_width::UnicodeWidthStr;
use values::SettingsView;

use crate::{
    overlay::{
        modal::{
            metrics::ModalRowStyle,
            placement::{
                ModalPlacement,
                ModalScrollAreas,
                OverlayAreas,
                OverlayContainer,
                column_width,
                leading_cells,
            },
        },
        settings::{
            rows::{SettingsColumns, SettingsTableRow, settings_row},
            values::{max_value_width, settings_label},
        },
    },
    primitive::{
        canvas::Canvas,
        glyphs::{self, TITLE_SEPARATOR},
        inset::Inset,
        list_chrome::scroll_offset,
        text::truncate_from_left,
    },
    theme::active_theme::ActiveTheme,
};

const LABEL_GAP: usize = 2;

#[derive(Debug)]
pub(crate) struct SettingsWidget<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) values: SettingsView<'a>,
    pub(crate) selected: usize,
    pub(crate) avoid: &'a [Rect],
}

impl<'a> SettingsWidget<'a> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.modal_title()).areas(screen))
    }

    fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
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
        self.paint_rows(&areas, buffer);
    }

    fn rows(&self) -> Vec<SettingRow> {
        SettingRow::all(self.values.appearance_settings)
    }

    fn modal_title(&self) -> String {
        modal_title_text(&self.values.music_dir_label(), self.content_width())
    }

    fn content_width(&self) -> u16 {
        settings_content_width(&self.rows(), &self.values)
    }

    fn placement<'title>(&self, modal_title: &'title str) -> ModalPlacement<'title>
    where
        'a: 'title,
    {
        ModalPlacement {
            inset: Inset::overlay(),
            container: OverlayContainer::Modal(self.avoid),
            border_title: Line::default(),
            modal_title,
            content_width: Cells(self.content_width()),
            content_rows: Cells(u16::try_from(self.rows().len()).unwrap_or(u16::MAX)),
            hint: None,
            theme: self.theme,
        }
    }

    fn paint_rows(&self, areas: &ModalScrollAreas, buffer: &mut Buffer) {
        let inner = areas.rows;
        let style = ModalRowStyle::from_theme(&self.theme);
        let rows = self.rows();
        let label_width = label_column_width(&rows);
        let columns = SettingsColumns::for_width(
            column_width(areas).0,
            leading_cells(areas).0,
            label_width,
        );
        let offset =
            scroll_offset(self.selected, rows.len(), usize::from(inner.height));
        let table = Table::new(
            rows.iter().map(|&row| {
                settings_row(
                    &SettingsTableRow {
                        row,
                        values: &self.values,
                        columns,
                    },
                    style,
                )
            }),
            columns.constraints(),
        )
        .column_spacing(0)
        .row_highlight_style(style.highlight());
        let mut table_rows = TableState::new()
            .with_offset(offset)
            .with_selected(Some(self.selected));
        StatefulWidget::render(table, inner, buffer, &mut table_rows);
    }
}

impl Widget for &SettingsWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

fn modal_title_text(music_dir: &str, content_width: u16) -> String {
    let fixed_width = glyphs::settings::TITLE_WORD.width() + TITLE_SEPARATOR.width();
    let path_budget = usize::from(content_width).saturating_sub(fixed_width);
    let path = truncate_from_left(music_dir, path_budget);
    format!("{}{TITLE_SEPARATOR}{path}", glyphs::settings::TITLE_WORD)
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
pub(crate) mod test_support {
    use std::path::Path;

    use config::appearance_file::TomlAppearance;
    use kernel::domain::{
        appearance::AppearanceSettings,
        crossfade::Crossfade,
        setting_row::AppearanceSetting,
        settings::ReplayGain,
    };

    use crate::overlay::settings::values::SettingsView;

    pub(crate) fn appearance_settings() -> Vec<AppearanceSetting> {
        kernel::domain::appearance_rows::appearance_settings(
            TomlAppearance::default().settings(),
        )
    }

    pub(crate) fn settings_values(
        appearance_settings: &[AppearanceSetting],
    ) -> SettingsView<'_> {
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
            appearance_settings,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::domain::{appearance::CoverMode, theme::ThemeName};
    use ratatui::layout::Rect;

    use crate::{
        overlay::{
            modal::{metrics::ModalRowStyle, placement::OverlayAreas},
            settings::{
                SettingsWidget,
                test_support::{appearance_settings, settings_values},
            },
        },
        primitive::canvas::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn outer_rect(overlay: &SettingsWidget<'_>, screen: Rect) -> Option<Rect> {
        match overlay.areas(screen) {
            OverlayAreas::List(areas) => Some(areas.outer),
            OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => None,
        }
    }

    #[test]
    fn settings_overlay_lists_every_row_with_its_label_and_value() {
        let theme = noir();
        let custom = appearance_settings();
        let overlay = SettingsWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn settings_overlay_highlights_the_selected_row() {
        let theme = noir();
        let custom = appearance_settings();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let overlay = SettingsWidget {
            theme: active,
            values: settings_values(&custom),
            selected: 1 + custom.len(),
            avoid: &[],
        };
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let selection_bg = ModalRowStyle::from_theme(&active).selected_background;
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
        let custom = appearance_settings();
        let overlay = SettingsWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "noir").is_some());
        assert!(find_text(&buffer, "Cover mode").is_some());
    }

    #[test]
    fn settings_title_keeps_the_path_tail_visible_at_a_narrow_width() {
        let theme = noir();
        let custom = appearance_settings();
        let mut with_long_path = settings_values(&custom);
        with_long_path.music_dir =
            Path::new("/Users/testuser/Music/Library/Deeply/Nested/Folder/apple-music");
        let overlay = SettingsWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: with_long_path,
            selected: 0,
            avoid: &[],
        };
        let buffer =
            rendered(60, 19, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "apple-music").is_some());
    }

    #[test]
    fn the_modal_outer_rect_stays_put_across_a_theme_and_an_appearance_change() {
        let theme = noir();
        let custom = appearance_settings();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let themes = [
            ThemeName::from_static("noir"),
            ThemeName::from_static("gruvbox-light"),
        ];
        let screen = Rect::new(0, 0, 80, 28);

        let mut with_noir = settings_values(&custom);
        with_noir.themes = &themes;
        with_noir.theme = "noir";
        let overlay_noir = SettingsWidget {
            theme: active,
            values: with_noir,
            selected: 0,
            avoid: &[],
        };

        let mut with_gruvbox = settings_values(&custom);
        with_gruvbox.themes = &themes;
        with_gruvbox.theme = "gruvbox-light";
        with_gruvbox.appearance.cover_mode = CoverMode::Off;
        let overlay_gruvbox = SettingsWidget {
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
        let custom = appearance_settings();
        let overlay = SettingsWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            values: settings_values(&custom),
            selected: 0,
            avoid: &[],
        };
        let frame = rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()))
            .to_string();
        assert_eq!(frame.lines().count(), 3);
    }
}
