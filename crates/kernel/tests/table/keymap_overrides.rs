use kernel::{
    cmd::Effect,
    domain::{
        cue::Cue,
        cursor_over::CursorOver,
        key::KeyPress,
        keymap::{Action, KeyContext, KeyOverride, KeymapOverrides},
        model::Model,
        overlay::Overlay,
        time::Moment,
        workspace::Workspace,
    },
    message::{ConfigEvent, Message, PlaybackRequest},
    update::keymap::{bindings::Keymap, lookup::route},
};
use rstest::rstest;

use crate::support::{
    keymap::{bindings, character},
    update::update,
};

fn reports_an_error(keymap_overrides: &KeymapOverrides) -> bool {
    let mut model = Model::default();
    let reload_event = ConfigEvent::KeymapReloaded(Box::new(keymap_overrides.clone()));
    update(&mut model, Message::Config(reload_event), Moment::default())
        .unwrap()
        .effects()
        .any(|effect| *effect == Effect::Animate(Cue::ToastRaised))
}

fn config_with(next: Option<&str>, prev: Option<&str>) -> KeymapOverrides {
    [(Action::Next, next), (Action::Previous, prev)]
        .into_iter()
        .filter_map(|(action, chord)| Some((action, KeyOverride::from(chord?))))
        .collect()
}

fn compiled(keymap_overrides: KeymapOverrides) -> Workspace {
    let mut workspace = Workspace::default();
    workspace.keymap = Keymap::new(keymap_overrides);
    workspace
}

#[rstest]
#[case::an_override_binds_its_own_chord(
    (Some("y"), None),
    'y',
    (Some(Message::Playback(PlaybackRequest::Next)), false)
)]
#[case::an_override_leaves_every_other_action_alone(
    (Some("y"), None),
    'p',
    (Some(Message::Playback(PlaybackRequest::Previous)), false)
)]
#[case::an_override_vacates_the_default_it_left((Some("y"), None), 'n', (None, false))]
#[case::no_override_keeps_the_default(
    (None, None),
    'n',
    (Some(Message::Playback(PlaybackRequest::Next)), false)
)]
#[case::a_malformed_override_falls_back_to_the_default(
    (Some("not-a-key"), None),
    'n',
    (Some(Message::Playback(PlaybackRequest::Next)), true)
)]
#[case::an_override_onto_a_default_wins_the_chord(
    (Some("p"), None),
    'p',
    (Some(Message::Playback(PlaybackRequest::Next)), true)
)]
#[case::two_overrides_on_one_chord_go_to_the_first_declared(
    (Some("y"), Some("y")),
    'y',
    (Some(Message::Playback(PlaybackRequest::Next)), true)
)]
#[case::the_loser_of_a_collision_reverts_to_its_own_default(
    (Some("y"), Some("y")),
    'p',
    (Some(Message::Playback(PlaybackRequest::Previous)), true)
)]
fn an_override_routes(
    #[case] chords: (Option<&str>, Option<&str>),
    #[case] pressed: char,
    #[case] outcome: (Option<Message>, bool),
) {
    let (expected, expects_an_error) = outcome;
    let config = config_with(chords.0, chords.1);
    assert_eq!(reports_an_error(&config), expects_an_error);
    let key = character(pressed);
    let press = KeyPress { key, typed: key };
    assert_eq!(route(&compiled(config), press), expected);
}

#[test]
fn an_override_that_takes_a_default_leaves_its_action_unbound() {
    let config = config_with(Some("p"), None);
    assert!(!bindings(&config).iter().any(|binding| {
        matches!(
            binding.message,
            Message::Playback(PlaybackRequest::Previous)
        )
    }));
}

fn next_in_search() -> KeymapOverrides {
    KeymapOverrides::from([(
        Action::Next,
        KeyOverride {
            chord: "n".to_string(),
            key_context: KeyContext::Search,
        },
    )])
}

fn plays_next() -> Option<Message> {
    Some(Message::Playback(PlaybackRequest::Next))
}

#[test]
fn a_binding_that_names_a_context_compiles_into_that_focus() {
    let config = next_in_search();
    assert!(!reports_an_error(&config));
    let moved = bindings(&config).into_iter().find(|binding| {
        matches!(binding.message, Message::Playback(PlaybackRequest::Next))
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
    workspace.keymap = Keymap::new(next_in_search());
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
