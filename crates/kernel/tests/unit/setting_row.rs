use kernel::domain::{
    Choice,
    CustomControl,
    CustomRow,
    CustomSetting,
    Direction,
    SettingId,
    SettingRow,
};

fn rows() -> Vec<SettingRow> {
    SettingRow::all(&[])
}

fn custom(id: u16) -> CustomSetting {
    let custom: &'static CustomRow = Box::leak(Box::new(CustomRow {
        id: SettingId::new(id),
        control: CustomControl::Toggle,
        cue: None,
        themes: &[],
    }));
    CustomSetting {
        custom,
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
    let custom_settings = [custom(1), custom(2)];
    let rows = SettingRow::all(&custom_settings);
    let cursor = SettingRow::Custom(SettingId::new(2));

    let kept = cursor.kept(&rows);

    assert_eq!(kept, SettingRow::Custom(SettingId::new(2)));
}

#[test]
fn falls_back_when_custom_row_gone() {
    let custom_settings = [custom(1)];
    let rows = SettingRow::all(&custom_settings);
    let cursor = SettingRow::Custom(SettingId::new(99));

    let kept = cursor.kept(&rows);

    assert_eq!(kept, rows[0]);
}
