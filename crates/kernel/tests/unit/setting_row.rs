use kernel::domain::{
    appearance_rows::APPEARANCE_ROWS,
    setting_row::{AppearanceField, SettingRow},
};

#[test]
fn position_answers_the_place_of_the_first_and_the_last_row_in_all() {
    let last = *SettingRow::ALL.last().unwrap();

    assert_eq!(SettingRow::first().position(), 0);
    assert_eq!(last.position(), SettingRow::ALL.len() - 1);
    assert!(
        SettingRow::ALL
            .into_iter()
            .enumerate()
            .all(|(place, row)| row.position() == place)
    );
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
