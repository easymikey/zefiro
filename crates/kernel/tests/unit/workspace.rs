use kernel::{
    cmd::{Cmd, Cue, Effect},
    domain::{
        config::{ConfigError, ConfigName, Diagnostic},
        io_error::IoError,
        keymap::{Action, KeyOverride, KeymapOverrides},
        model::Model,
        revision::Revision,
        theme::ThemeName,
        time::Moment,
        toast::{TOAST_LIFETIME, Toast},
    },
    message::{ConfigEvent, ConfigReload, Message, Timer},
};
use rstest::rstest;

use crate::support::{first_toast_expiry, step::update};

fn reduce(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
}

fn theme() -> ConfigName {
    ConfigName::Theme(ThemeName::from_static("noir"))
}

fn fail(source: ConfigName, text: &str) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload {
        name: source,
        result: Err(ConfigError::Invalid(Diagnostic::from_error(
            &std::io::Error::other(text.to_string()),
        ))),
    })
}

fn recovered(source: ConfigName) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload {
        name: source,
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
fn a_shown_toast_schedules_its_expiry_after_the_one_lifetime() {
    let mut model = Model::default();

    let cmd = reduce(&mut model, Message::Toast(Toast::error("boom")));

    assert!(cmd.effects().any(|effect| *effect
        == Effect::After {
            delay: TOAST_LIFETIME,
            timer: Timer::Toast(model.revisions.toast),
        }));
    assert_ne!(model.revisions.toast, Revision::default());
}

#[test]
fn a_source_failing_again_with_the_same_words_does_not_raise_a_second_toast() {
    let mut model = Model::default();

    let first = reduce(&mut model, Message::Config(fail(theme(), "Theme: boom")));
    model.workspace.toasts.clear();
    let repeat = reduce(&mut model, Message::Config(fail(theme(), "Theme: boom")));
    let changed = reduce(&mut model, Message::Config(fail(theme(), "Theme: worse")));

    assert!(has_raised_a_toast(&first), "{first:?}");
    assert!(repeat == Cmd::none());
    assert!(has_raised_a_toast(&changed), "{changed:?}");
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|toast| toast.text.clone()),
        Some("Theme: worse".to_string())
    );
}

#[test]
fn keys_reloaded_installs_the_merged_table() {
    let mut model = Model::default();
    let config = KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]);
    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::KeymapReloaded(Box::new(config.clone()))),
    );
    assert_eq!(model.workspace.keymap.overrides(), &config);
    assert!(cmd == Cmd::none());
}

#[rstest]
#[case::a_failing_source_shows_its_own_text(
    &[Message::Config(fail(theme(), "Theme: boom"))],
    Some("Theme: boom")
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
        Message::Config(fail(ConfigName::Appearance, "UI: broken")),
        Message::Config(recovered(theme())),
    ],
    Some("UI: broken")
)]
#[case::recovery_without_a_failure_disturbs_nothing(
    &[Message::Config(recovered(theme()))],
    None
)]
fn config_errors_decide_which_toast_is_on_screen(
    #[case] requests: &[Message],
    #[case] expected: Option<&str>,
) {
    let mut model = Model::default();

    for request in requests {
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
fn a_config_failure_shows_the_kernels_own_words() {
    let mut model = Model::default();

    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(ConfigError::Unreadable {
            file: ConfigName::Appearance,
            kind: IoError::Denied,
        })),
    );

    assert!(has_raised_a_toast(&cmd), "{cmd:?}");
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|toast| toast.text.clone()),
        Some("the appearance file is unreadable: permission denied".to_string())
    );
}

#[test]
fn a_repeated_config_failure_still_raises_its_own_toast() {
    let mut model = Model::default();
    let failure = ConfigError::Save {
        file: ConfigName::Config,
        kind: IoError::Full,
    };

    let _first = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(failure.clone())),
    );
    model.workspace.toasts.clear();
    let second = reduce(&mut model, Message::Config(ConfigEvent::Error(failure)));

    assert!(
        has_raised_a_toast(&second),
        "unlike a source failure, a config failure is not deduplicated"
    );
}

#[test]
fn config_failures_word_each_kind_distinctly() {
    let unreadable = ConfigError::ThemesUnreadable(IoError::Other);
    let save = ConfigError::Save {
        file: theme(),
        kind: IoError::Full,
    };

    insta::assert_debug_snapshot!((unreadable.to_string(), save.to_string()));
}

#[test]
fn theme_reloaded_leaves_the_toast_alone() {
    let mut model = Model::default();
    model.workspace.toasts = vec![Toast::error("Theme: boom")];

    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir"))),
    );

    assert_eq!(model.workspace.toasts, vec![Toast::error("Theme: boom")]);
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ThemeChanged)))
    );
}
