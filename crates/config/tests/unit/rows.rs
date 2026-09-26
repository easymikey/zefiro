use config::{appearance_patch, appearance_row};
use kernel::domain::{OptionCount, SettingId};
use rstest::rstest;

#[rstest]
#[case::cover_style("cover_style", 0, 2)]
#[case::key_hints("key_hints", 6, 1)]
#[case::layout_mode("layout_mode", 11, 2)]
fn a_row_patches_through_the_public_contract(
    #[case] name: &str,
    #[case] id: u16,
    #[case] position: usize,
) {
    let option = appearance_row(SettingId::new(id))
        .unwrap()
        .spec
        .control
        .count()
        .index(position)
        .unwrap();
    let patch = appearance_patch(SettingId::new(id), option).unwrap();
    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_debug_snapshot!(patch);
    });
}

#[test]
fn an_unknown_row_is_rejected_through_the_public_contract() {
    let option = OptionCount::new(1).unwrap().index(0).unwrap();
    let rejected = appearance_patch(SettingId::new(999), option);
    insta::assert_debug_snapshot!(rejected);
}
