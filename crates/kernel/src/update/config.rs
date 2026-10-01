use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, ScanMode, WindowColorsCmd},
    domain::{CustomSetting, Model, Overlay, SettingRow, ThemeName, Toast},
    message::ConfigEvent,
    update::{error::UpdateError, workspace::SourceOutcome},
};

pub(crate) fn update(
    model: &mut Model,
    event: ConfigEvent,
) -> Result<Cmd, UpdateError> {
    match event {
        ConfigEvent::KeymapReloaded(keys) => {
            let changed = model.workspace.keymap.overrides() != &*keys;
            let cmd = model.workspace.keymap_reloaded(*keys, &mut model.revisions);
            if changed {
                let _ = model.revisions.config.bump();
            }
            Ok(cmd)
        }
        ConfigEvent::ThemeReloaded(name) => Ok(theme_reloaded(model, name)),
        ConfigEvent::ThemesLoaded(themes) => {
            model.themes.names = themes;
            Ok(Cmd::None)
        }
        ConfigEvent::MusicDirReloaded(reloaded) => {
            Ok(music_dir_reloaded(model, reloaded))
        }
        ConfigEvent::CustomSettingsReloaded(settings) => {
            custom_settings_reloaded(model, settings);
            Ok(Cmd::None)
        }
        ConfigEvent::SourceFailed { source, text } => {
            Ok(model.workspace.source_result(
                SourceOutcome {
                    source,
                    text: Some(text),
                },
                &mut model.revisions,
            ))
        }
        ConfigEvent::SourceRecovered(source) => {
            Ok(model.workspace.source_recovered(source))
        }
        ConfigEvent::Error(failure) => Ok(model
            .workspace
            .show(Toast::error(failure.to_string()), &mut model.revisions)),
    }
}

fn theme_reloaded(model: &mut Model, name: ThemeName) -> Cmd {
    let _ = model.revisions.theme.bump();
    Cmd::from(Effect::WindowColors(WindowColorsCmd::Apply(name)))
        .then(Cue::ThemeChanged.into())
}

fn custom_settings_reloaded(model: &mut Model, settings: Vec<CustomSetting>) {
    model.custom_settings = settings;
    let Model {
        workspace,
        custom_settings,
        ..
    } = model;
    let Some(Overlay::Settings { selected }) = &mut workspace.overlay else {
        return;
    };
    *selected = selected.kept(&SettingRow::all(custom_settings));
}

fn music_dir_reloaded(model: &mut Model, reloaded: std::path::PathBuf) -> Cmd {
    if reloaded == model.music_dir {
        Cmd::None
    } else {
        model.music_dir = reloaded.clone();
        Effect::Library(LibraryCmd::Scan {
            music_dir: reloaded,
            revision: model.revisions.issue_scan(),
            mode: ScanMode::Full,
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        cmd::{Cmd, Effect, LibraryCmd},
        domain::{
            Action,
            ConfigError,
            KeyOverride,
            KeymapOverrides,
            Model,
            ThemeName,
            Themes,
            ToastLevel,
        },
        message::ConfigEvent,
        update::config::update,
    };

    #[test]
    fn keymap_reload_bumps_the_config_revision() {
        let mut model = Model::default();
        let before = model.revisions.config;

        let keys = KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]);
        let _ =
            update(&mut model, ConfigEvent::KeymapReloaded(Box::new(keys))).unwrap();

        assert_ne!(model.revisions.config, before);
    }

    #[test]
    fn unchanged_keymap_keeps_the_revision() {
        let mut model = Model::default();
        let keys = model.workspace.keymap.overrides().clone();
        let _ = update(
            &mut model,
            ConfigEvent::KeymapReloaded(Box::new(keys.clone())),
        )
        .unwrap();
        let before = model.revisions.config;

        let _ =
            update(&mut model, ConfigEvent::KeymapReloaded(Box::new(keys))).unwrap();

        assert_eq!(model.revisions.config, before);
    }

    #[test]
    fn theme_reload_bumps_the_theme_revision_and_cues() {
        let mut model = Model::default();
        let before = model.revisions.theme;

        let cmd = update(
            &mut model,
            ConfigEvent::ThemeReloaded(ThemeName::from_static("noir")),
        )
        .unwrap();

        assert_ne!(model.revisions.theme, before);
        assert!(matches!(cmd, Cmd::Batch(_)));
    }

    #[test]
    fn a_watch_error_raises_the_error_toast() {
        let mut model = Model::default();

        let _ = update(
            &mut model,
            ConfigEvent::Error(ConfigError::Watch {
                detail: "x".to_string(),
            }),
        )
        .unwrap();

        let toast = model.workspace.toast.unwrap();
        assert_eq!(toast.level, ToastLevel::Error);
        assert_eq!(toast.text, "Config watch failed: x");
    }

    fn rescanned_music_dir(cmd: &Cmd) -> Option<PathBuf> {
        match cmd {
            Cmd::One(Effect::Library(LibraryCmd::Scan { music_dir, .. })) => {
                Some(music_dir.clone())
            }
            Cmd::None | Cmd::One(_) | Cmd::Batch(_) => None,
        }
    }

    #[rstest::rstest]
    #[case::the_root_it_already_plays(PathBuf::from("/music"), None)]
    #[case::another_root(PathBuf::from("/other"), Some(PathBuf::from("/other")))]
    fn a_music_dir_reload_rescans_only_a_music_dir_that_moved(
        #[case] reloaded: PathBuf,
        #[case] expected: Option<PathBuf>,
    ) {
        let mut model = Model {
            music_dir: PathBuf::from("/music"),
            ..Model::default()
        };

        let cmd = update(&mut model, ConfigEvent::MusicDirReloaded(reloaded.clone()))
            .unwrap();

        assert_eq!(rescanned_music_dir(&cmd), expected);
        assert_eq!(model.music_dir, reloaded);
    }

    #[test]
    fn setting_themes_installs_the_list_the_shell_found() {
        let mut model = Model {
            themes: Themes {
                names: vec![ThemeName::from_static("noir")],
                ..Themes::default()
            },
            ..Model::default()
        };

        let cmd = update(
            &mut model,
            ConfigEvent::ThemesLoaded(vec![ThemeName::from_static("wafer")]),
        )
        .unwrap();

        assert_eq!(model.themes.names, [ThemeName::from_static("wafer")]);
        assert!(matches!(cmd, Cmd::None));
    }
}
