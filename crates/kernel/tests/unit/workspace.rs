use kernel::{
    Cmd,
    Cue,
    Effect,
    Message,
    Timer,
    WorkspaceRequest,
    domain::{
        Action,
        ConfigFailure,
        ConfigFile,
        ConfigSource,
        KeyOverride,
        KeymapOverrides,
        Model,
        Revision,
        TOAST_LIFETIME,
        Toast,
    },
    update::update,
};
use rstest::rstest;

use crate::support::first_toast_expiry;

fn reduce(model: &mut Model, request: WorkspaceRequest) -> Cmd {
    update(model, Message::Workspace(request)).unwrap()
}

fn fail(source: ConfigSource, text: &str) -> WorkspaceRequest {
    WorkspaceRequest::SourceFailed {
        source,
        text: text.to_string(),
    }
}

fn recovered(source: ConfigSource) -> WorkspaceRequest {
    WorkspaceRequest::SourceRecovered(source)
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
    let cmd = reduce(&mut model, WorkspaceRequest::ShowToast(toast.clone()));
    assert_eq!(model.workspace.toast, Some(toast));
    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );

    let cleared = reduce(&mut model, WorkspaceRequest::ClearToast);
    assert!(model.workspace.toast.is_none());
    assert!(matches!(cleared, Cmd::None));
}

#[test]
fn a_shown_toast_schedules_its_expiry_after_the_one_lifetime() {
    let mut model = Model::default();

    let cmd = reduce(
        &mut model,
        WorkspaceRequest::ShowToast(Toast::error("boom".into())),
    );

    assert!(cmd.effects().any(|effect| *effect
        == Effect::After {
            delay: TOAST_LIFETIME,
            message: Timer::Toast(model.toast_generation),
        }));
    assert_ne!(model.toast_generation, Revision::UNSTAMPED);
}

#[test]
fn a_source_failing_again_with_the_same_words_does_not_raise_a_second_toast() {
    let mut model = Model::default();

    let first = reduce(&mut model, fail(ConfigSource::Theme, "Theme: boom"));
    model.workspace.toast = None;
    let repeat = reduce(&mut model, fail(ConfigSource::Theme, "Theme: boom"));
    let changed = reduce(&mut model, fail(ConfigSource::Theme, "Theme: worse"));

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
        WorkspaceRequest::KeymapReloaded(Box::new(config.clone())),
    );
    assert_eq!(model.workspace.keymap.config(), &config);
    assert!(matches!(cmd, Cmd::None));
}

#[rstest]
#[case::a_failing_source_shows_its_own_text(
    &[fail(ConfigSource::Theme, "Theme: boom")],
    Some("Theme: boom")
)]
#[case::recovery_clears_the_toast_it_put_up(
    &[fail(ConfigSource::Theme, "Theme: boom"), recovered(ConfigSource::Theme)],
    None
)]
#[case::recovery_clears_without_reviving_another_sources_words(
    &[
        fail(ConfigSource::Keymap, "Keymap: bad chord"),
        fail(ConfigSource::Theme, "Theme: boom"),
        recovered(ConfigSource::Theme),
    ],
    None
)]
#[case::recovery_of_a_source_nobody_is_showing_keeps_the_toast(
    &[
        fail(ConfigSource::Theme, "Theme: boom"),
        fail(ConfigSource::Appearance, "UI: broken"),
        recovered(ConfigSource::Theme),
    ],
    Some("UI: broken")
)]
#[case::recovery_without_a_failure_disturbs_nothing(&[recovered(ConfigSource::Theme)], None)]
fn source_errors_decide_which_toast_is_on_screen(
    #[case] requests: &[WorkspaceRequest],
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
        WorkspaceRequest::ConfigFailed(ConfigFailure::Unreadable {
            file: ConfigFile::Appearance,
            detail: "permission denied".to_string(),
        }),
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
    let failure = ConfigFailure::Save {
        file: ConfigFile::Keymap,
        detail: "disk full".to_string(),
    };

    let _first = reduce(&mut model, WorkspaceRequest::ConfigFailed(failure.clone()));
    model.workspace.toast = None;
    let second = reduce(&mut model, WorkspaceRequest::ConfigFailed(failure));

    assert!(
        has_raised_a_toast(&second),
        "unlike a source failure, a config failure is not deduplicated"
    );
}

#[test]
fn config_failures_word_each_kind_distinctly() {
    let unreadable = ConfigFailure::Unreadable {
        file: ConfigFile::ThemeDirectory,
        detail: "not a directory".to_string(),
    };
    let save = ConfigFailure::Save {
        file: ConfigFile::Theme,
        detail: "disk full".to_string(),
    };

    insta::assert_debug_snapshot!((unreadable.to_string(), save.to_string()));
}

#[test]
fn theme_reloaded_leaves_the_toast_alone() {
    let mut model = Model::default();
    model.workspace.toast = Some(Toast::error("Theme: boom".into()));

    let cmd = reduce(&mut model, WorkspaceRequest::ThemeReloaded);

    assert_eq!(
        model.workspace.toast,
        Some(Toast::error("Theme: boom".into()))
    );
    assert!(matches!(cmd, Cmd::One(Effect::Animate(Cue::ThemeChanged))));
}
