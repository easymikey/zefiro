use std::path::PathBuf;

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, ScanMode, WindowColorsCmd},
    domain::{
        overlay::Overlay,
        revision::Revisions,
        setting_row::{AppearanceSetting, SettingRow},
        settings::Settings,
        theme::{ThemeName, Themes},
        toast::Toast,
        workspace::Workspace,
    },
    message::ConfigEvent,
    update::machine::Unhandled,
};

pub(crate) struct ConfigParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) settings: &'a mut Settings,
    pub(crate) themes: &'a mut Themes,
    pub(crate) appearance_settings: &'a mut Vec<AppearanceSetting>,
    pub(crate) music_dir: &'a mut PathBuf,
}

pub(crate) fn update(
    config: ConfigParts<'_>,
    event: ConfigEvent,
) -> Result<Cmd, Unhandled> {
    let ConfigParts {
        workspace,
        revisions,
        settings,
        themes,
        appearance_settings,
        music_dir,
    } = config;
    match event {
        ConfigEvent::KeymapReloaded(keys) => {
            let changed = workspace.keymap.overrides() != &*keys;
            let cmd = workspace.keymap_reloaded(*keys, revisions);
            if changed {
                revisions.config.advance();
            }
            Ok(cmd)
        }
        ConfigEvent::ThemeReloaded(name) => Ok(theme_reloaded(revisions, name)),
        ConfigEvent::AppearanceReloaded(appearance) => {
            settings.appearance = appearance;
            Ok(Cmd::none())
        }
        ConfigEvent::ThemesLoaded(names) => {
            themes.names = names;
            Ok(Cmd::none())
        }
        ConfigEvent::MusicDirReloaded(reloaded) => {
            Ok(music_dir_reloaded(music_dir, revisions, reloaded))
        }
        ConfigEvent::AppearanceSettingsReloaded(reloaded) => {
            appearance_settings_reloaded(workspace, appearance_settings, reloaded);
            Ok(Cmd::none())
        }
        ConfigEvent::Reloaded(reload) => {
            Ok(workspace.config_reloaded(reload, revisions))
        }
        ConfigEvent::Error(failure) => Ok(workspace.show(
            Toast::error("Config error").with_text(failure.to_string()),
            revisions,
        )),
    }
}

fn theme_reloaded(revisions: &mut Revisions, name: ThemeName) -> Cmd {
    revisions.theme.advance();
    Cmd::from(Effect::WindowColors(WindowColorsCmd::Set(name)))
        .then(Cue::ThemeChanged.into())
}

fn appearance_settings_reloaded(
    workspace: &mut Workspace,
    appearance_settings: &mut Vec<AppearanceSetting>,
    reloaded: Vec<AppearanceSetting>,
) {
    *appearance_settings = reloaded;
    let Some(Overlay::Settings(selected)) = &mut workspace.overlay else {
        return;
    };
    *selected = selected.kept(&SettingRow::all(appearance_settings));
}

fn music_dir_reloaded(
    music_dir: &mut PathBuf,
    revisions: &mut Revisions,
    reloaded: PathBuf,
) -> Cmd {
    if reloaded == *music_dir {
        Cmd::none()
    } else {
        *music_dir = reloaded.clone();
        Effect::Library(LibraryCmd::Scan {
            music_dir: reloaded,
            revision: revisions.issue_scan(),
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
            config::ConfigError,
            keymap::{Action, KeyOverride, KeymapOverrides},
            model::Model,
            theme::{ThemeName, Themes},
            toast::ToastKind,
        },
        message::ConfigEvent,
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

        let keys = KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]);
        update(&mut model, ConfigEvent::KeymapReloaded(Box::new(keys))).unwrap();

        assert_ne!(model.revisions.config, before);
    }

    #[test]
    fn unchanged_keymap_keeps_the_revision() {
        let mut model = Model::default();
        let keys = model.workspace.keymap.overrides().clone();
        update(
            &mut model,
            ConfigEvent::KeymapReloaded(Box::new(keys.clone())),
        )
        .unwrap();
        let before = model.revisions.config;

        update(&mut model, ConfigEvent::KeymapReloaded(Box::new(keys))).unwrap();

        assert_eq!(model.revisions.config, before);
    }

    #[test]
    fn theme_reload_bumps_the_theme_revision_and_cues() {
        let mut model = Model::default();
        let before = model.revisions.theme;

        let cmd = emitted(
            &mut model,
            ConfigEvent::ThemeReloaded(ThemeName::from_static("noir")),
        );

        assert_ne!(model.revisions.theme, before);
        assert_ne!(cmd, Cmd::none());
    }

    #[test]
    fn a_watch_error_raises_the_error_toast() {
        let mut model = Model::default();

        update(
            &mut model,
            ConfigEvent::Error(ConfigError::Watch(
                crate::domain::io_error::IoError::Other,
            )),
        )
        .unwrap();

        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.kind, ToastKind::Error);
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

        let cmd = emitted(&mut model, ConfigEvent::MusicDirReloaded(reloaded.clone()));

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

        let cmd = emitted(
            &mut model,
            ConfigEvent::ThemesLoaded(vec![ThemeName::from_static("wafer")]),
        );

        assert_eq!(model.themes.names, [ThemeName::from_static("wafer")]);
        assert!(cmd == Cmd::none());
    }
}
