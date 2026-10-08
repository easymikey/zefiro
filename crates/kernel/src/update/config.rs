use std::path::PathBuf;

use crate::{
    cmd::{Cmd, Effect, LibraryCmd, ScanMode, WindowColorsCmd},
    domain::{
        cue::Cue,
        revision::Revisions,
        settings::Settings,
        theme::{ThemeName, Themes},
        toast::Toast,
        workspace::Workspace,
    },
    message::ConfigEvent,
    update::machine::{Unhandled, replace},
};

pub(crate) struct ConfigParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) settings: &'a mut Settings,
    pub(crate) themes: &'a mut Themes,
    pub(crate) music_dir: &'a mut PathBuf,
}

pub(crate) fn update(
    config_parts: ConfigParts<'_>,
    event: ConfigEvent,
) -> Result<Cmd, Unhandled> {
    let ConfigParts {
        workspace,
        revisions,
        settings,
        themes,
        music_dir,
    } = config_parts;
    match event {
        ConfigEvent::KeymapReloaded(keymap_overrides) => {
            workspace.keymap_reloaded(*keymap_overrides, revisions)
        }
        ConfigEvent::ThemeReloaded(name) => Ok(theme_reloaded(revisions, name)),
        ConfigEvent::AppearanceReloaded(appearance) => {
            replace(&mut settings.appearance_settings, appearance).map(|()| Cmd::none())
        }
        ConfigEvent::ThemesLoaded {
            theme_names: names,
            refused,
        } => {
            if refused.is_empty() {
                return replace(&mut themes.names, names).map(|()| Cmd::none());
            }
            themes.names = names;
            Ok(workspace.show(
                Toast::info(format!(
                    "Skipped themes with invalid names: {}",
                    refused.join(", ")
                )),
                revisions,
            ))
        }
        ConfigEvent::MusicDirReloaded(reloaded) => {
            music_dir_reloaded(music_dir, revisions, reloaded)
        }
        ConfigEvent::Reloaded(reload) => workspace.config_reloaded(reload, revisions),
        ConfigEvent::Error(error) => Ok(workspace.show(
            Toast::error("Config error").with_text(error.to_string()),
            revisions,
        )),
    }
}

fn theme_reloaded(revisions: &mut Revisions, name: ThemeName) -> Cmd {
    revisions.theme.advance();
    Cmd::from(Effect::WindowColors(WindowColorsCmd::Set(name)))
        .then(Cue::ThemeChanged.into())
}

fn music_dir_reloaded(
    music_dir: &mut PathBuf,
    revisions: &mut Revisions,
    reloaded_path: PathBuf,
) -> Result<Cmd, Unhandled> {
    replace(music_dir, reloaded_path.clone())?;
    Ok(Effect::Library(LibraryCmd::Scan {
        music_dir: reloaded_path,
        revision: revisions.issue_scan(),
        mode: ScanMode::Fresh,
    })
    .into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect, LibraryCmd},
        domain::{
            appearance::{AppearanceSettings, CoverMode},
            config::{ConfigError, ConfigName},
            cue::Cue,
            io_error::IoError,
            keymap::{Action, KeyOverride, KeymapOverrides},
            model::Model,
            settings::Settings,
            theme::{ThemeName, Themes},
            toast::{Toast, ToastLevel},
        },
        message::{ConfigEvent, ConfigReload},
        update::{config, config_parts, machine::Unhandled},
    };

    fn update(model: &mut Model, event: ConfigEvent) -> Result<(), Unhandled> {
        config::update(config_parts(model), event).map(drop)
    }

    fn emitted(model: &mut Model, event: ConfigEvent) -> Cmd {
        config::update(config_parts(model), event).unwrap()
    }

    #[test]
    fn keymap_reload_bumps_the_config_revision() {
        let mut model = Model::default();
        let before = model.revisions.config;

        let keymap_overrides =
            KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]);
        let cmd = emitted(
            &mut model,
            ConfigEvent::KeymapReloaded(Box::new(keymap_overrides.clone())),
        );

        assert_ne!(model.revisions.config, before);
        assert_eq!(model.workspace.keymap.overrides(), &keymap_overrides);
        assert_eq!(cmd, Cmd::none());
    }

    #[test]
    fn theme_reload_bumps_the_theme_revision_and_cues() {
        let mut model = Model::default();
        model.workspace.toasts = vec![Toast::error("Theme: boom")];
        let before = model.revisions.theme;

        let cmd = emitted(
            &mut model,
            ConfigEvent::ThemeReloaded(ThemeName::from_static("noir")),
        );

        assert_ne!(model.revisions.theme, before);
        assert_eq!(model.workspace.toasts, vec![Toast::error("Theme: boom")]);
        assert!(
            cmd.effects()
                .any(|effect| matches!(effect, Effect::Animate(Cue::ThemeChanged)))
        );
    }

    #[test]
    fn a_watch_error_raises_the_error_toast() {
        let mut model = Model::default();

        update(
            &mut model,
            ConfigEvent::Error(ConfigError::Watch(IoError::Other)),
        )
        .unwrap();

        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.level, ToastLevel::Error);
        assert_eq!(
            toast.text.as_deref(),
            Some("Config watch failed: an unknown error")
        );
    }

    fn rescanned_music_dir(cmd: &Cmd) -> Option<PathBuf> {
        match cmd.effects().as_slice() {
            [Effect::Library(LibraryCmd::Scan { music_dir, .. })] => {
                Some(music_dir.clone())
            }
            _ => None,
        }
    }

    #[test]
    fn a_music_dir_reload_rescans_only_a_music_dir_that_moved() {
        let mut model = Model {
            music_dir: PathBuf::from("/music"),
            ..Model::default()
        };

        let cmd = emitted(
            &mut model,
            ConfigEvent::MusicDirReloaded(PathBuf::from("/other")),
        );

        assert_eq!(rescanned_music_dir(&cmd), Some(PathBuf::from("/other")));
        assert_eq!(model.music_dir, PathBuf::from("/other"));
    }

    #[test]
    fn a_repeated_config_error_is_refused() {
        let mut model = Model::default();
        let event = ConfigEvent::Reloaded(ConfigReload {
            name: ConfigName::Config,
            result: Err(ConfigError::Read {
                name: ConfigName::Config,
                error: IoError::Denied,
            }),
        });
        update(&mut model, event.clone()).unwrap();
        let before = model.clone();

        let result = update(&mut model, event);

        assert_eq!(result, Err(Unhandled));
        assert_eq!(model, before);
    }

    struct RefusalRow {
        model: Model,
        event: ConfigEvent,
    }

    #[rstest]
    #[case::an_unchanged_keymap(RefusalRow {
        model: Model::default(),
        event: ConfigEvent::KeymapReloaded(Box::new(
            Model::default().workspace.keymap.overrides().clone(),
        )),
    })]
    #[case::an_unchanged_music_dir(RefusalRow {
        model: Model {
            music_dir: PathBuf::from("/music"),
            ..Model::default()
        },
        event: ConfigEvent::MusicDirReloaded(PathBuf::from("/music")),
    })]
    #[case::a_config_reload_with_nothing_to_clear(RefusalRow {
        model: Model::default(),
        event: ConfigEvent::Reloaded(ConfigReload {
            name: ConfigName::Config,
            result: Ok(()),
        }),
    })]
    #[case::an_equal_appearance(RefusalRow {
        model: Model {
            settings: Settings {
                appearance_settings: AppearanceSettings {
                    cover_mode: CoverMode::Plain,
                    ..AppearanceSettings::default()
                },
                ..Settings::default()
            },
            ..Model::default()
        },
        event: ConfigEvent::AppearanceReloaded(AppearanceSettings {
            cover_mode: CoverMode::Plain,
            ..AppearanceSettings::default()
        }),
    })]
    #[case::the_same_themes(RefusalRow {
        model: Model {
            themes: Themes {
                names: vec![ThemeName::from_static("wafer")],
                ..Themes::default()
            },
            ..Model::default()
        },
        event: ConfigEvent::ThemesLoaded {
            theme_names: vec![ThemeName::from_static("wafer")],
            refused: Vec::new(),
        },
    })]
    fn a_reload_that_changes_nothing_is_refused(#[case] row: RefusalRow) {
        let RefusalRow { mut model, event } = row;
        let before = model.clone();

        let result = update(&mut model, event);

        assert_eq!(result, Err(Unhandled));
        assert_eq!(model, before);
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

        let cmd = emitted(
            &mut model,
            ConfigEvent::ThemesLoaded {
                theme_names: vec![ThemeName::from_static("wafer")],
                refused: Vec::new(),
            },
        );

        assert_eq!(model.themes.names, [ThemeName::from_static("wafer")]);
        assert!(cmd == Cmd::none());
        assert!(model.workspace.toasts.is_empty());
    }

    #[test]
    fn a_theme_list_with_refused_names_raises_one_info_toast() {
        let mut model = Model::default();

        update(
            &mut model,
            ConfigEvent::ThemesLoaded {
                theme_names: vec![ThemeName::from_static("wafer")],
                refused: vec!["solar..dark".to_string(), "auto".to_string()],
            },
        )
        .unwrap();

        let [toast] = model.workspace.toasts.as_slice() else {
            panic!("{:?}", model.workspace.toasts);
        };
        assert_eq!(toast.level, ToastLevel::Info);
        assert_eq!(
            toast.title,
            "Skipped themes with invalid names: solar..dark, auto"
        );
    }
}
