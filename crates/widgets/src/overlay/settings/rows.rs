use kernel::domain::SettingRow;
use ratatui::{layout::Constraint, style::Style, text::Line, widgets::Row};

use crate::{
    overlay::{
        modal::{ModalRowColors, indented},
        settings::values::{SettingsView, settings_label, value_text},
    },
    primitive::{glyphs::TruncateGlyphs, text::truncate_to_width},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsColumns {
    label: u16,
    value: u16,
    lead: u16,
}

impl SettingsColumns {
    pub(crate) fn for_width(width: u16, lead: u16, label_width: usize) -> Self {
        let label = small_width(label_width).saturating_add(lead);
        Self {
            label,
            value: width.saturating_sub(label),
            lead,
        }
    }

    pub(crate) fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label),
            Constraint::Length(self.value),
        ]
    }
}

fn small_width(width: usize) -> u16 {
    u16::try_from(width).unwrap_or(u16::MAX)
}

pub(crate) struct SettingsRowView<'a> {
    pub(crate) row: SettingRow,
    pub(crate) values: &'a SettingsView<'a>,
    pub(crate) columns: SettingsColumns,
}

pub(crate) fn settings_row(
    view: &SettingsRowView<'_>,
    colors: ModalRowColors,
) -> Row<'static> {
    let [label, value] = settings_cells(view);
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.text)),
        Line::from(value).style(Style::default().fg(colors.text)),
    ])
}

fn settings_cells(view: &SettingsRowView<'_>) -> [String; 2] {
    let columns = view.columns;
    let cell = |value: &str, width: u16| {
        truncate_to_width(value, usize::from(width), TruncateGlyphs::default())
            .into_owned()
    };
    [
        indented(settings_label(view.row), columns.lead, columns.label),
        cell(&value_text(view.row, view.values), columns.value),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::domain::{CustomSetting, SettingRow};
    use unicode_width::UnicodeWidthStr;

    use crate::{
        overlay::settings::rows::{SettingsColumns, SettingsRowView, settings_cells},
        scene::fixtures::{custom_settings, settings_values},
    };

    fn all_rows(custom_settings: &[CustomSetting]) -> Vec<SettingRow> {
        SettingRow::all(custom_settings)
    }

    #[test]
    fn every_row_fits_its_columns() {
        let custom = custom_settings();
        let values = settings_values(&custom);
        let columns = SettingsColumns::for_width(60, 0, 20);
        for row in all_rows(&custom) {
            let [label, value] = settings_cells(&SettingsRowView {
                row,
                values: &values,
                columns,
            });
            assert!(
                label.width() <= usize::from(columns.label),
                "row {row:?} label {label:?} overflows its column"
            );
            assert!(
                value.width() <= usize::from(columns.value),
                "row {row:?} value {value:?} overflows its column"
            );
        }
    }
}
