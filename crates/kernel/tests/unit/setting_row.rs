use kernel::domain::{
    AppearanceControl,
    AppearanceRow,
    AppearanceSetting,
    Choice,
    Direction,
    SettingRow,
    appearance_rows::AppearanceField,
};

fn rows() -> Vec<SettingRow> {
    SettingRow::all(&[])
}

fn custom(id: AppearanceField) -> AppearanceSetting {
    let row: &'static AppearanceRow = Box::leak(Box::new(AppearanceRow {
        field: id,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    }));
    AppearanceSetting {
        row,
        choice: Choice::Mixed,
    }
}

#[test]
fn down_from_first_moves_to_the_second_row() {
    let cursor = SettingRow::Theme;

    let moved = cursor.moved(&rows(), Direction::Next);

    assert_eq!(moved, rows()[1]);
}

#[test]
fn up_from_first_stays_on_the_first_row() {
    let cursor = SettingRow::Theme;

    let moved = cursor.moved(&rows(), Direction::Previous);

    assert_eq!(moved, rows()[0]);
}

#[test]
fn down_from_last_stays_on_the_last_row() {
    let all = rows();
    let cursor = *all.last().unwrap();

    let moved = cursor.moved(&all, Direction::Next);

    assert_eq!(moved, *all.last().unwrap());
}

#[test]
fn kept_when_row_still_there() {
    let appearance_settings = [
        custom(AppearanceField::CoverBrackets),
        custom(AppearanceField::CoverStyle),
    ];
    let rows = SettingRow::all(&appearance_settings);
    let cursor = SettingRow::Appearance(AppearanceField::CoverStyle);

    let kept = cursor.kept(&rows);

    assert_eq!(kept, SettingRow::Appearance(AppearanceField::CoverStyle));
}

#[test]
fn falls_back_when_custom_row_gone() {
    let appearance_settings = [custom(AppearanceField::CoverBrackets)];
    let rows = SettingRow::all(&appearance_settings);
    let cursor = SettingRow::Appearance(AppearanceField::Animations);

    let kept = cursor.kept(&rows);

    assert_eq!(kept, rows[0]);
}
