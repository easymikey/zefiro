use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        config::{ConfigError, ConfigName, Diagnostic},
        cue::Cue,
        io_error::IoError,
        keymap::{Action, KeyOverride, KeymapOverrides},
        model::Model,
        theme::ThemeName,
        time::Moment,
        toast::Toast,
    },
    message::{ConfigEvent, ConfigReload, Message},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    first_toast_expiry,
    update::{send, update},
};

fn reduce(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
}

fn theme() -> ConfigName {
    ConfigName::Theme(ThemeName::from_static("noir"))
}

fn fail(config_name: ConfigName, text: &str) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload {
        name: config_name,
        result: Err(
            Diagnostic::from_error(&std::io::Error::other(text.to_string())).into(),
        ),
    })
}

fn recovered(config_name: ConfigName) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload {
        name: config_name,
        result: Ok(()),
    })
}

fn has_raised_a_toast(cmd: &Cmd) -> bool {
    matches!(
        cmd.effects().as_slice(),
        [Effect::Animate(Cue::ToastRaised), Effect::After { .. }]
    )
}

#[test]
fn showing_a_toast_installs_it_and_clearing_takes_it_away() {
    let mut model = Model::default();
    assert!(model.workspace.toasts.is_empty());

    let toast = Toast::error("boom");
    let cmd = reduce(&mut model, Message::Toast(toast.clone()));
    assert_eq!(model.workspace.toasts, vec![toast]);
    assert_eq!(
        cmd,
        Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])
    );
}

#[test]
fn a_bad_chord_keeps_its_toast_through_the_config_reload_that_follows() {
    let mut model = Model::default();
    let overrides =
        KeymapOverrides::from([(Action::PlayPause, KeyOverride::from("bad"))]);
    send(
        &mut model,
        Message::Config(ConfigEvent::KeymapReloaded(Box::new(overrides))),
    );
    send(&mut model, Message::Config(recovered(ConfigName::Config)));

    let toast = model.workspace.toasts.first();
    assert_eq!(
        toast.map(|toast| toast.title.clone()),
        Some(format!("Trouble with {}", ConfigName::Config))
    );
    assert!(toast.is_some_and(|toast| toast.text.is_some()), "{toast:?}");
    assert_eq!(
        update(
            &mut model,
            Message::Config(recovered(ConfigName::Config)),
            Moment::default(),
        ),
        Err(Unhandled)
    );
}

#[rstest]
#[case::a_failing_source_shows_its_own_text(
    &[Message::Config(fail(theme(), "Theme: boom"))],
    Some("Theme: boom")
)]
#[case::a_source_failing_again_with_new_words_replaces_its_text(
    &[
        Message::Config(fail(theme(), "Theme: boom")),
        Message::Config(fail(theme(), "Theme: worse")),
    ],
    Some("Theme: worse")
)]
#[case::recovery_clears_the_toast_it_put_up(
    &[
        Message::Config(fail(theme(), "Theme: boom")),
        Message::Config(recovered(theme())),
    ],
    None
)]
#[case::recovery_uncovers_the_toast_beneath(
    &[
        Message::Config(fail(ConfigName::Config, "Keymap: bad chord")),
        Message::Config(fail(theme(), "Theme: boom")),
        Message::Config(recovered(theme())),
    ],
    Some("Keymap: bad chord")
)]
#[case::recovery_of_a_source_nobody_is_showing_keeps_the_toast(
    &[
        Message::Config(fail(theme(), "Theme: boom")),
        Message::Config(fail(ConfigName::Config, "Config: broken")),
        Message::Config(recovered(theme())),
    ],
    Some("Config: broken")
)]
fn config_errors_decide_which_toast_is_on_screen(
    #[case] messages: &[Message],
    #[case] expected: Option<&str>,
) {
    let mut model = Model::default();

    for request in messages {
        let _cmd = reduce(&mut model, request.clone());
    }

    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|toast| toast.text.as_deref()),
        expected
    );
}

#[test]
fn a_config_error_shows_the_kernels_own_words() {
    let mut model = Model::default();

    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(ConfigError::Read {
            name: ConfigName::Config,
            error: IoError::Denied,
        })),
    );

    assert!(has_raised_a_toast(&cmd), "{cmd:?}");
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|toast| toast.text.clone()),
        Some("the config file is unreadable: permission denied".to_string())
    );
}

#[test]
fn a_repeated_config_error_still_raises_its_own_toast() {
    let mut model = Model::default();
    let error = ConfigError::Save {
        name: ConfigName::Config,
        error: IoError::Full,
    };

    let _first = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(error.clone())),
    );
    model.workspace.toasts.clear();
    let second = reduce(&mut model, Message::Config(ConfigEvent::Error(error)));

    assert!(
        has_raised_a_toast(&second),
        "unlike a source failure, a config failure is not deduplicated"
    );
}

#[test]
fn config_errors_word_each_error_distinctly() {
    let unreadable_error = ConfigError::ListThemes(IoError::Other);
    let save_error = ConfigError::Save {
        name: theme(),
        error: IoError::Full,
    };

    insta::assert_debug_snapshot!((
        unreadable_error.to_string(),
        save_error.to_string()
    ));
}
