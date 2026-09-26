use kernel::domain::{
    Choice,
    CustomControl,
    CustomSetting,
    CustomSpec,
    Nudge,
    SettingId,
    SettingRow,
    SettingsCursor,
};

fn rows() -> Vec<SettingRow> {
    SettingRow::all(&[])
}

fn custom(id: u16) -> CustomSetting {
    let spec: &'static CustomSpec = Box::leak(Box::new(CustomSpec {
        id: SettingId::new(id),
        control: CustomControl::Toggle,
        cue: None,
        themes: &[],
    }));
    CustomSetting {
        spec,
        choice: Choice::Mixed,
    }
}

#[test]
fn down_from_first_moves_to_the_second_row() {
    let cursor = SettingsCursor {
        selected: SettingRow::Theme,
    };

    let moved = cursor.moved(&rows(), Nudge::Down);

    assert_eq!(moved.selected, rows()[1]);
}

#[test]
fn up_from_first_stays_on_the_first_row() {
    let cursor = SettingsCursor {
        selected: SettingRow::Theme,
    };

    let moved = cursor.moved(&rows(), Nudge::Up);

    assert_eq!(moved.selected, rows()[0]);
}

#[test]
fn down_from_last_stays_on_the_last_row() {
    let all = rows();
    let cursor = SettingsCursor {
        selected: *all.last().unwrap(),
    };

    let moved = cursor.moved(&all, Nudge::Down);

    assert_eq!(moved.selected, *all.last().unwrap());
}

#[test]
fn kept_when_row_still_there() {
    let custom_rows = [custom(1), custom(2)];
    let rows = SettingRow::all(&custom_rows);
    let cursor = SettingsCursor {
        selected: SettingRow::Custom(SettingId::new(2)),
    };

    let kept = cursor.kept(&rows);

    assert_eq!(kept.selected, SettingRow::Custom(SettingId::new(2)));
}

#[test]
fn falls_back_when_custom_row_gone() {
    let custom_rows = [custom(1)];
    let rows = SettingRow::all(&custom_rows);
    let cursor = SettingsCursor {
        selected: SettingRow::Custom(SettingId::new(99)),
    };

    let kept = cursor.kept(&rows);

    assert_eq!(kept.selected, rows[0]);
}
