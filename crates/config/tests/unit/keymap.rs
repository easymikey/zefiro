use config::KeymapFile;
use kernel::{
    Key,
    KeyCode,
    KeyPress,
    Message,
    PlaybackRequest,
    domain::{
        Action,
        CursorOver,
        KeyContext,
        KeyOverride,
        KeyValidationError,
        Keymap,
        KeymapOverrides,
        Overlay,
        Workspace,
    },
    update::keymap::{Bindings, KeyBinding, KeyOutcome, default_bindings, route},
};
use rstest::rstest;

fn character(letter: char) -> Key {
    Key::plain(KeyCode::Char(letter))
}

fn bindings(config: &KeymapOverrides) -> Vec<KeyBinding> {
    Bindings::new(config).as_slice().to_vec()
}

fn validation_errors(config: &KeymapOverrides) -> Vec<KeyValidationError> {
    Keymap::new(config.clone(), &default_bindings())
        .errors()
        .to_vec()
}

fn config_with(next: Option<&str>, prev: Option<&str>) -> KeymapOverrides {
    [(Action::Next, next), (Action::Prev, prev)]
        .into_iter()
        .filter_map(|(action, chord)| Some((action, KeyOverride::from(chord?))))
        .collect()
}

fn compiled(config: KeymapOverrides) -> Workspace {
    let mut workspace = Workspace::default();
    workspace.bindings = Bindings::new(&config);
    workspace.keymap = Keymap::new(config, &default_bindings());
    workspace
}

struct PressedUnder {
    config: KeymapOverrides,
    pressed: char,
}

fn pressed_under(
    next: Option<&str>,
    prev: Option<&str>,
    pressed: char,
) -> PressedUnder {
    PressedUnder {
        config: config_with(next, prev),
        pressed,
    }
}

#[rstest]
#[case::an_override_binds_its_own_chord(
    pressed_under(Some("y"), None, 'y'),
    Some(Message::Playback(PlaybackRequest::Next)),
    false
)]
#[case::an_override_leaves_every_other_action_alone(
    pressed_under(Some("y"), None, 'p'),
    Some(Message::Playback(PlaybackRequest::Prev)),
    false
)]
#[case::an_override_vacates_the_default_it_left(
    pressed_under(Some("y"), None, 'n'),
    None,
    false
)]
#[case::no_override_keeps_the_default(
    pressed_under(None, None, 'n'),
    Some(Message::Playback(PlaybackRequest::Next)),
    false
)]
#[case::a_malformed_override_falls_back_to_the_default(
    pressed_under(Some("not-a-key"), None, 'n'),
    Some(Message::Playback(PlaybackRequest::Next)),
    true
)]
#[case::an_override_onto_a_default_wins_the_chord(
    pressed_under(Some("p"), None, 'p'),
    Some(Message::Playback(PlaybackRequest::Next)),
    true
)]
#[case::two_overrides_on_one_chord_go_to_the_first_declared(
    pressed_under(Some("y"), Some("y"), 'y'),
    Some(Message::Playback(PlaybackRequest::Next)),
    true
)]
#[case::the_loser_of_a_collision_reverts_to_its_own_default(
    pressed_under(Some("y"), Some("y"), 'p'),
    Some(Message::Playback(PlaybackRequest::Prev)),
    true
)]
fn an_override_routes(
    #[case] under: PressedUnder,
    #[case] expected: Option<Message>,
    #[case] reports_an_error: bool,
) {
    assert_eq!(
        !validation_errors(&under.config).is_empty(),
        reports_an_error
    );
    let key = character(under.pressed);
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&compiled(under.config), press), expected);
}

#[test]
fn an_override_that_takes_a_default_leaves_its_action_unbound() {
    let config = config_with(Some("p"), None);
    assert!(!bindings(&config).iter().any(|binding| {
        matches!(
            binding.outcome,
            KeyOutcome::Message(Message::Playback(PlaybackRequest::Prev))
        )
    }));
}

fn next_in_search() -> KeymapOverrides {
    KeymapOverrides::from([(
        Action::Next,
        KeyOverride {
            chord: "n".to_string(),
            context: KeyContext::Search,
        },
    )])
}

fn plays_next() -> Option<Message> {
    Some(Message::Playback(PlaybackRequest::Next))
}

#[test]
fn a_binding_that_names_a_context_compiles_into_that_focus() {
    let config = next_in_search();
    assert!(validation_errors(&config).is_empty());
    let moved = bindings(&config).into_iter().find(|binding| {
        matches!(
            binding.outcome,
            KeyOutcome::Message(Message::Playback(PlaybackRequest::Next))
        )
    });
    assert_eq!(
        moved.map(|binding| binding.key_context),
        Some(KeyContext::Search)
    );
}

#[rstest]
#[case::it_fires_inside_the_overlay_it_names(
    Overlay::Search(CursorOver::default()),
    plays_next()
)]
#[case::it_types_nothing_it_took_the_chord_from(Overlay::Help, None)]
fn a_binding_that_names_a_context_routes_only_there(
    #[case] overlay: Overlay,
    #[case] expected: Option<Message>,
) {
    let mut workspace = Workspace::default();
    workspace.overlay = Some(overlay);
    workspace.bindings = Bindings::new(&next_in_search());
    let key = character('n');
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&workspace, press), expected);
}

#[test]
fn a_binding_that_names_a_context_leaves_the_playlist_without_it() {
    let workspace = compiled(next_in_search());
    let key = character('n');
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&workspace, press), None);
}

#[rstest]
#[case::a_string_is_the_short_form_for_the_global_context(
    "next = \"y\"",
    Some(KeyOverride::from("y"))
)]
#[case::a_table_names_the_context(
    "next = { chord = \"y\", context = \"search\" }",
    Some(KeyOverride { chord: String::from("y"), context: KeyContext::Search })
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
fn a_keys_entry_parses(#[case] spelling: &str, #[case] expected: Option<KeyOverride>) {
    let parsed = toml::from_str::<KeymapFile>(spelling)
        .ok()
        .map(KeymapOverrides::from)
        .and_then(|file| file.binding(Action::Next).cloned());
    assert_eq!(parsed, expected);
}

#[test]
fn the_shipped_keymap_has_no_chord_collisions() {
    assert!(validation_errors(&KeymapOverrides::default()).is_empty());
}
