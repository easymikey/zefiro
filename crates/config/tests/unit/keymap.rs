use config::config_file::parse_config_settings;
use kernel::domain::keymap::{Action, KeyContext, KeyOverride};
use rstest::rstest;

#[rstest]
#[case::a_string_keeps_the_action_context(
    "next = \"y\"",
    KeyOverride { chord: String::from("y"), key_context: None }
)]
#[case::a_table_names_the_context(
    "next = { chord = \"y\", context = \"search\" }",
    KeyOverride { chord: String::from("y"), key_context: Some(KeyContext::Search) }
)]
#[case::a_table_without_a_context_keeps_the_action_context(
    "next = { chord = \"y\" }",
    KeyOverride { chord: String::from("y"), key_context: None }
)]
fn a_key_binding_reads_as_a_chord_with_its_context(
    #[case] spelling: &str,
    #[case] expected: KeyOverride,
) {
    let settings = parse_config_settings(&format!("[keymap]\n{spelling}\n")).unwrap();
    assert_eq!(settings.keymap_overrides.get(Action::Next), Some(&expected));
}

#[rstest]
#[case::an_unknown_context_does_not_parse(
    "next = { chord = \"y\", context = \"nowhere\" }"
)]
#[case::an_unknown_field_does_not_parse("next = { chord = \"y\", scope = \"search\" }")]
#[case::a_table_without_a_chord_does_not_parse("next = { context = \"search\" }")]
#[case::an_unknown_action_does_not_parse("nekst = \"y\"")]
fn a_bad_key_binding_does_not_parse(#[case] spelling: &str) {
    assert!(parse_config_settings(&format!("[keymap]\n{spelling}\n")).is_err());
}
