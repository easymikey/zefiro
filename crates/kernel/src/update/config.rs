use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, WindowColorsCmd},
    domain::{
        CustomSetting,
        Model,
        Overlay,
        Revision,
        ScanMode,
        SettingRow,
        ThemeName,
        Toast,
        WindowColors,
    },
    message::ConfigEvent,
    update::{error::UpdateError, workspace::SourceOutcome},
};

pub(crate) fn update(
    model: &mut Model,
    event: ConfigEvent,
) -> Result<Cmd, UpdateError> {
    match event {
        ConfigEvent::KeymapReloaded(keys) => {
            let changed = model.workspace.keymap.config() != &*keys;
            let cmd = model.workspace.keymap_reloaded(*keys);
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
            Ok(music_dir_reloaded(&mut model.music_dir, reloaded))
        }
        ConfigEvent::CustomRowsReloaded(rows) => {
            reload_custom_rows(model, rows);
            Ok(Cmd::None)
        }
        ConfigEvent::SourceFailed { source, text } => {
            Ok(model.workspace.source_result(SourceOutcome {
                source,
                text: Some(text),
            }))
        }
        ConfigEvent::SourceRecovered(source) => {
            Ok(model.workspace.source_recovered(source))
        }
        ConfigEvent::Error(failure) => {
            Ok(model.workspace.show(Toast::error(failure.to_string())))
        }
    }
}

fn theme_reloaded(model: &mut Model, name: ThemeName) -> Cmd {
    let _ = model.revisions.theme.bump();
    let window_colors = window_colors_change(&name);
    model.window_colors = WindowColors::Themed(name);
    Cmd::from(Effect::WindowColors(window_colors)).then(Cue::ThemeChanged.into())
}

pub(crate) fn window_colors_change(current: &ThemeName) -> WindowColorsCmd {
    WindowColorsCmd::Apply(current.clone())
}

fn reload_custom_rows(model: &mut Model, rows: Vec<CustomSetting>) {
    model.custom_settings = rows;
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

fn music_dir_reloaded(
    music_dir: &mut std::path::PathBuf,
    reloaded: std::path::PathBuf,
) -> Cmd {
    if reloaded == *music_dir {
        Cmd::None
    } else {
        *music_dir = reloaded.clone();
        Effect::Library(LibraryCmd::Scan {
            music_dir: reloaded,
            revision: Revision::UNSTAMPED,
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
        let keys = model.workspace.keymap.config().clone();
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
    fn watch_failure_raises_the_error_toast() {
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

    fn rescanned_root(cmd: &Cmd) -> Option<PathBuf> {
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
    fn a_music_dir_reload_rescans_only_a_root_that_moved(
        #[case] reloaded: PathBuf,
        #[case] expected: Option<PathBuf>,
    ) {
        let mut model = Model {
            music_dir: PathBuf::from("/music"),
            ..Model::default()
        };

        let cmd = update(&mut model, ConfigEvent::MusicDirReloaded(reloaded.clone()))
            .unwrap();

        assert_eq!(rescanned_root(&cmd), expected);
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

    #[rstest::rstest]
    #[case::noir_applies("noir")]
    #[case::solar_applies("solar")]
    fn window_colors_follow_a_theme_reload(#[case] current: &str) {
        let current = ThemeName::new(current.to_string()).unwrap();

        let applied = crate::update::config::window_colors_change(&current);

        assert_eq!(
            match applied {
                crate::cmd::WindowColorsCmd::Apply(name) => name.to_string(),
                crate::cmd::WindowColorsCmd::Reset => String::new(),
            },
            current.to_string()
        );
    }
}
