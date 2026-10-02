use config::parse_config_reload;
use kernel::domain::{Action, KeyContext, KeyOverride};
use rstest::rstest;

#[rstest]
#[case::a_string_is_the_short_form_for_the_global_context(
    "next = \"y\"",
    Some(KeyOverride::from("y"))
)]
#[case::a_table_names_the_context(
    "next = { chord = \"y\", context = \"search\" }",
    Some(KeyOverride { chord: String::from("y"), key_context: KeyContext::Search })
)]
#[case::a_table_without_a_context_is_global(
    "next = { chord = \"y\" }",
    Some(KeyOverride::from("y"))
)]
#[case::an_unknown_context_does_not_parse(
    "next = { chord = \"y\", context = \"nowhere\" }",
    None
)]
#[case::an_unknown_field_does_not_parse(
    "next = { chord = \"y\", scope = \"search\" }",
    None
)]
#[case::a_table_without_a_chord_does_not_parse("next = { context = \"search\" }", None)]
#[case::an_unknown_action_does_not_parse("nekst = \"y\"", None)]
fn a_key_binding_reads_as_a_chord_with_its_context(
    #[case] spelling: &str,
    #[case] expected: Option<KeyOverride>,
) {
    let parsed = parse_config_reload(&format!("[keymap]\n{spelling}\n"))
        .ok()
        .and_then(|reload| reload.keymap.get(Action::Next).cloned());
    assert_eq!(parsed, expected);
}
