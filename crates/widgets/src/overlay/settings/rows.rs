use kernel::domain::{geometry::Cells, setting_row::SettingRow};
use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    text::Line,
    widgets::Row,
};

use crate::{
    overlay::{
        modal::placement::indented,
        settings::view::{SettingsView, settings_label, value_text},
    },
    primitive::text::truncate,
    theme::colors::Colors,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsColumns {
    label: Cells,
    value: Cells,
    lead: Cells,
}

impl SettingsColumns {
    pub(crate) fn for_width(width: Cells, lead: Cells, label_width: Cells) -> Self {
        let label = Cells(label_width.0.saturating_add(lead.0));
        Self {
            label,
            value: Cells(width.0.saturating_sub(label.0)),
            lead,
        }
    }

    pub(crate) fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label.0),
            Constraint::Length(self.value.0),
        ]
    }
}

pub(crate) struct SettingsTableRow<'a> {
    pub(crate) row: SettingRow,
    pub(crate) view: &'a SettingsView<'a>,
    pub(crate) columns: SettingsColumns,
}

pub(crate) fn settings_row(
    table_row: &SettingsTableRow<'_>,
    colors: Colors<Color>,
) -> Row<'static> {
    let [label, value] = settings_cells(table_row);
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.text)),
        Line::from(value).style(Style::default().fg(colors.text)),
    ])
}

fn settings_cells(table_row: &SettingsTableRow<'_>) -> [String; 2] {
    let columns = table_row.columns;
    let cell = |value: &str, width: Cells| truncate(value, width.count()).into_owned();
    [
        indented(settings_label(table_row.row), columns.lead, columns.label),
        cell(&value_text(table_row.row, table_row.view), columns.value),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        geometry::Cells,
        setting_row::{AppearanceSetting, SettingRow},
    };
    use unicode_width::UnicodeWidthStr;

    use crate::overlay::settings::{
        rows::{SettingsColumns, SettingsTableRow, settings_cells},
        test_support::{appearance_rows, settings_values},
    };

    fn all_rows(appearance_rows: &[AppearanceSetting]) -> Vec<SettingRow> {
        SettingRow::all(appearance_rows)
    }

    #[test]
    fn every_row_fits_its_columns() {
        let custom = appearance_rows();
        let view = settings_values();
        let columns = SettingsColumns::for_width(Cells(60), Cells(0), Cells(20));
        for row in all_rows(&custom) {
            let [label, value] = settings_cells(&SettingsTableRow {
                row,
                view: &view,
                columns,
            });
            assert!(
                label.width() <= columns.label.count(),
                "row {row:?} label {label:?} overflows its column"
            );
            assert!(
                value.width() <= columns.value.count(),
                "row {row:?} value {value:?} overflows its column"
            );
        }
    }
}
