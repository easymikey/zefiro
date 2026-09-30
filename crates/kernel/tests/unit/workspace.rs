use kernel::{
    Cmd,
    ConfigEvent,
    Cue,
    Effect,
    Message,
    Moment,
    Timer,
    WorkspaceRequest,
    domain::{
        Action,
        ConfigError,
        ConfigFile,
        KeyOverride,
        KeymapOverrides,
        Model,
        Revision,
        TOAST_LIFETIME,
        ThemeName,
        Toast,
    },
    update::update,
};
use rstest::rstest;

use crate::support::first_toast_expiry;

fn reduce(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
}

fn fail(source: ConfigFile, text: &str) -> ConfigEvent {
    ConfigEvent::SourceFailed {
        source,
        text: text.to_string(),
    }
}

fn recovered(source: ConfigFile) -> ConfigEvent {
    ConfigEvent::SourceRecovered(source)
}

fn has_raised_a_toast(cmd: &Cmd) -> bool {
    matches!(
        cmd,
        Cmd::Batch(effects) if matches!(
            effects.as_slice(),
            [Effect::Animate(Cue::ToastRaised), Effect::After { .. }]
        )
    )
}

#[test]
fn showing_a_toast_installs_it_and_clearing_takes_it_away() {
    let mut model = Model::default();
    assert!(model.workspace.toast.is_none());

    let toast = Toast::error("boom".into());
    let cmd = reduce(
        &mut model,
        Message::Workspace(WorkspaceRequest::ShowToast(toast.clone())),
    );
    assert_eq!(model.workspace.toast, Some(toast));
    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );

    let cleared = reduce(&mut model, Message::Workspace(WorkspaceRequest::ClearToast));
    assert!(model.workspace.toast.is_none());
    assert!(matches!(cleared, Cmd::None));
}

#[test]
fn a_shown_toast_schedules_its_expiry_after_the_one_lifetime() {
    let mut model = Model::default();

    let cmd = reduce(
        &mut model,
        Message::Workspace(WorkspaceRequest::ShowToast(Toast::error("boom".into()))),
    );

    assert!(cmd.effects().any(|effect| *effect
        == Effect::After {
            delay: TOAST_LIFETIME,
            message: Timer::Toast(model.revisions.toast),
        }));
    assert_ne!(model.revisions.toast, Revision::UNSTAMPED);
}

#[test]
fn a_source_failing_again_with_the_same_words_does_not_raise_a_second_toast() {
    let mut model = Model::default();

    let first = reduce(
        &mut model,
        Message::Config(fail(ConfigFile::Theme, "Theme: boom")),
    );
    model.workspace.toast = None;
    let repeat = reduce(
        &mut model,
        Message::Config(fail(ConfigFile::Theme, "Theme: boom")),
    );
    let changed = reduce(
        &mut model,
        Message::Config(fail(ConfigFile::Theme, "Theme: worse")),
    );

    assert!(has_raised_a_toast(&first), "{first:?}");
    assert!(matches!(repeat, Cmd::None));
    assert!(has_raised_a_toast(&changed), "{changed:?}");
    assert_eq!(
        model.workspace.toast.map(|toast| toast.text),
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
    assert_eq!(model.workspace.keymap.config(), &config);
    assert!(matches!(cmd, Cmd::None));
}

#[rstest]
#[case::a_failing_source_shows_its_own_text(
    &[Message::Config(fail(ConfigFile::Theme, "Theme: boom"))],
    Some("Theme: boom")
)]
#[case::recovery_clears_the_toast_it_put_up(
    &[
        Message::Config(fail(ConfigFile::Theme, "Theme: boom")),
        Message::Config(recovered(ConfigFile::Theme)),
    ],
    None
)]
#[case::recovery_clears_without_reviving_another_sources_words(
    &[
        Message::Config(fail(ConfigFile::Config, "Keymap: bad chord")),
        Message::Config(fail(ConfigFile::Theme, "Theme: boom")),
        Message::Config(recovered(ConfigFile::Theme)),
    ],
    None
)]
#[case::recovery_of_a_source_nobody_is_showing_keeps_the_toast(
    &[
        Message::Config(fail(ConfigFile::Theme, "Theme: boom")),
        Message::Config(fail(ConfigFile::Appearance, "UI: broken")),
        Message::Config(recovered(ConfigFile::Theme)),
    ],
    Some("UI: broken")
)]
#[case::recovery_without_a_failure_disturbs_nothing(
    &[Message::Config(recovered(ConfigFile::Theme))],
    None
)]
fn source_errors_decide_which_toast_is_on_screen(
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
            .toast
            .as_ref()
            .map(|toast| toast.text.as_str()),
        expected
    );
}

#[test]
fn a_config_failure_shows_the_kernels_own_words() {
    let mut model = Model::default();

    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(ConfigError::Unreadable {
            file: ConfigFile::Appearance,
            detail: "permission denied".to_string(),
        })),
    );

    assert!(has_raised_a_toast(&cmd), "{cmd:?}");
    assert_eq!(
        model.workspace.toast.map(|toast| toast.text),
        Some("the appearance file is unreadable: permission denied".to_string())
    );
}

#[test]
fn a_repeated_config_failure_still_raises_its_own_toast() {
    let mut model = Model::default();
    let failure = ConfigError::Save {
        file: ConfigFile::Config,
        detail: "disk full".to_string(),
    };

    let _first = reduce(
        &mut model,
        Message::Config(ConfigEvent::Error(failure.clone())),
    );
    model.workspace.toast = None;
    let second = reduce(&mut model, Message::Config(ConfigEvent::Error(failure)));

    assert!(
        has_raised_a_toast(&second),
        "unlike a source failure, a config failure is not deduplicated"
    );
}

#[test]
fn config_failures_word_each_kind_distinctly() {
    let unreadable = ConfigError::ThemesUnreadable {
        detail: "not a directory".to_string(),
    };
    let save = ConfigError::Save {
        file: ConfigFile::Theme,
        detail: "disk full".to_string(),
    };

    insta::assert_debug_snapshot!((unreadable.to_string(), save.to_string()));
}

#[test]
fn theme_reloaded_leaves_the_toast_alone() {
    let mut model = Model::default();
    model.workspace.toast = Some(Toast::error("Theme: boom".into()));

    let cmd = reduce(
        &mut model,
        Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir"))),
    );

    assert_eq!(
        model.workspace.toast,
        Some(Toast::error("Theme: boom".into()))
    );
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ThemeChanged)))
    );
}
