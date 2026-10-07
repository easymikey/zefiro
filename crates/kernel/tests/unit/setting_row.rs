use std::collections::HashSet;

use kernel::domain::{
    appearance_rows::APPEARANCE_ROWS,
    direction::Direction,
    setting_row::{AppearanceField, SettingRow},
};

#[test]
fn down_from_first_moves_to_the_second_row() {
    let setting_row = SettingRow::first();

    let moved = setting_row.moved(&SettingRow::ALL, Direction::Next);

    assert_eq!(moved, SettingRow::ALL[1]);
}

#[test]
fn up_from_first_stays_on_the_first_row() {
    let setting_row = SettingRow::first();

    let moved = setting_row.moved(&SettingRow::ALL, Direction::Previous);

    assert_eq!(moved, SettingRow::ALL[0]);
}

#[test]
fn down_from_last_stays_on_the_last_row() {
    let setting_row = *SettingRow::ALL.last().unwrap();

    let moved = setting_row.moved(&SettingRow::ALL, Direction::Next);

    assert_eq!(moved, setting_row);
}

#[test]
fn all_lists_every_row_once_starting_at_first() {
    let setting_rows: HashSet<SettingRow> = SettingRow::ALL.into_iter().collect();

    assert_eq!(setting_rows.len(), SettingRow::ALL.len());
    assert_eq!(SettingRow::ALL[0], SettingRow::first());
}

#[test]
fn all_lists_the_appearance_rows_in_their_table_order() {
    let appearance_fields: Vec<AppearanceField> = SettingRow::ALL
        .into_iter()
        .filter_map(|row| match row {
            SettingRow::Appearance(field) => Some(field),
            SettingRow::Theme
            | SettingRow::Crossfade
            | SettingRow::ReplayGain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => None,
        })
        .collect();
    let table_fields: Vec<AppearanceField> =
        APPEARANCE_ROWS.iter().map(|row| row.field).collect();

    assert_eq!(appearance_fields, table_fields);
}
