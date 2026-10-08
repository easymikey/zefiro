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
    primitive::truncate::truncate_owned,
    theme::colors::Colors,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsColumns {
    label_width: Cells,
    value_width: Cells,
    lead_width: Cells,
}

impl SettingsColumns {
    pub(crate) fn for_width(
        width: Cells,
        lead_width: Cells,
        label_width: Cells,
    ) -> Self {
        let label = Cells(label_width.0.saturating_add(lead_width.0));
        Self {
            label_width: label,
            value_width: Cells(width.0.saturating_sub(label.0)),
            lead_width,
        }
    }

    pub(crate) fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label_width.0),
            Constraint::Length(self.value_width.0),
        ]
    }
}

pub(crate) struct SettingsTableRow<'a> {
    pub(crate) setting_row: SettingRow,
    pub(crate) view: &'a SettingsView<'a>,
    pub(crate) columns: SettingsColumns,
}

pub(crate) fn settings_row(
    table_row: &SettingsTableRow<'_>,
    colors: Colors<Color>,
) -> Row<'static> {
    let [label, value] = settings_cells(table_row);
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.foreground)),
        Line::from(value).style(Style::default().fg(colors.foreground)),
    ])
}

fn settings_cells(table_row: &SettingsTableRow<'_>) -> [String; 2] {
    let columns = table_row.columns;
    [
        indented(
            settings_label(table_row.setting_row),
            columns.lead_width,
            columns.label_width,
        ),
        truncate_owned(
            value_text(table_row.setting_row, table_row.view),
            columns.value_width.count(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::domain::{geometry::Cells, setting_row::SettingRow};
    use unicode_width::UnicodeWidthStr;

    use crate::overlay::settings::{
        rows::{SettingsColumns, SettingsTableRow, settings_cells},
        tests::settings_values,
    };

    #[test]
    fn every_row_fits_its_columns() {
        let view = settings_values();
        let columns = SettingsColumns::for_width(Cells(60), Cells(0), Cells(20));
        for row in SettingRow::ALL {
            let [label, value] = settings_cells(&SettingsTableRow {
                setting_row: row,
                view: &view,
                columns,
            });
            assert!(
                label.width() <= columns.label_width.count(),
                "row {row:?} label {label:?} overflows its column"
            );
            assert!(
                value.width() <= columns.value_width.count(),
                "row {row:?} value {value:?} overflows its column"
            );
        }
    }
}
