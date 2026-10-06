use kernel::domain::{
    direction::Direction,
    setting_row::{
        AppearanceControl,
        AppearanceField,
        AppearanceRow,
        AppearanceRowChoice,
        Choice,
        SettingRow,
    },
};

fn rows() -> Vec<SettingRow> {
    SettingRow::all(&[])
}

fn custom(field: AppearanceField) -> AppearanceRowChoice {
    let row: &'static AppearanceRow = Box::leak(Box::new(AppearanceRow {
        field,
        control: AppearanceControl::Toggle,
        cue: None,
        theme_names: &[],
    }));
    AppearanceRowChoice {
        row,
        choice: Choice::Mixed,
    }
}

#[test]
fn down_from_first_moves_to_the_second_row() {
    let setting_row = SettingRow::Theme;

    let moved = setting_row.moved(&rows(), Direction::Next);

    assert_eq!(moved, rows()[1]);
}

#[test]
fn up_from_first_stays_on_the_first_row() {
    let setting_row = SettingRow::Theme;

    let moved = setting_row.moved(&rows(), Direction::Previous);

    assert_eq!(moved, rows()[0]);
}

#[test]
fn down_from_last_stays_on_the_last_row() {
    let all = rows();
    let setting_row = *all.last().unwrap();

    let moved = setting_row.moved(&all, Direction::Next);

    assert_eq!(moved, *all.last().unwrap());
}

#[test]
fn kept_when_row_still_there() {
    let appearance_row_choices = [
        custom(AppearanceField::CoverBrackets),
        custom(AppearanceField::CoverMode),
    ];
    let rows = SettingRow::all(&appearance_row_choices);
    let setting_row = SettingRow::Appearance(AppearanceField::CoverMode);

    let kept = setting_row.kept(&rows);

    assert_eq!(kept, SettingRow::Appearance(AppearanceField::CoverMode));
}

#[test]
fn falls_back_when_custom_row_gone() {
    let appearance_row_choices = [custom(AppearanceField::CoverBrackets)];
    let rows = SettingRow::all(&appearance_row_choices);
    let setting_row = SettingRow::Appearance(AppearanceField::Animations);

    let kept = setting_row.kept(&rows);

    assert_eq!(kept, rows[0]);
}
