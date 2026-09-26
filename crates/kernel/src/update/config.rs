use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, WindowColorsCmd},
    domain::{CustomSetting, Model, Overlay, Revision, SettingRow, ThemeName, Toast},
    message::ConfigFact,
    update::{
        rejection::Rejection,
        workspace::{KeymapReload, SourceOutcome},
    },
};

pub(crate) fn config(model: &mut Model, fact: ConfigFact) -> Result<Cmd, Rejection> {
    match fact {
        ConfigFact::KeymapReloaded(keys) => {
            let reload = model.workspace.keymap_reload(&keys);
            let cmd = model.workspace.keymap_reloaded(*keys);
            if let KeymapReload::Fresh = reload {
                let _ = model.config_generation.bump();
            }
            Ok(cmd)
        }
        ConfigFact::ThemeReloaded(name) => Ok(theme_reloaded(model, name)),
        ConfigFact::ThemesLoaded(themes) => {
            model.themes.names = themes;
            Ok(Cmd::None)
        }
        ConfigFact::MusicDirReloaded(reloaded) => {
            Ok(music_dir_reloaded(&mut model.music_dir, reloaded))
        }
        ConfigFact::CustomRowsReloaded(rows) => {
            reload_custom_rows(model, rows);
            Ok(Cmd::None)
        }
        ConfigFact::SourceFailed { source, text } => {
            Ok(model.workspace.source_result(SourceOutcome {
                source,
                text: Some(text),
            }))
        }
        ConfigFact::SourceRecovered(source) => {
            Ok(model.workspace.source_recovered(source))
        }
        ConfigFact::Failed(failure) => {
            Ok(model.workspace.show(Toast::error(failure.to_string())))
        }
    }
}

fn theme_reloaded(model: &mut Model, name: ThemeName) -> Cmd {
    let _ = model.theme_generation.bump();
    let window_colors = window_colors_change(model.window_colors.as_ref(), &name);
    model.window_colors = Some(name);
    window_colors
        .map_or(Cmd::None, |command| Effect::WindowColors(command).into())
        .then(Cue::ThemeChanged.into())
}

pub(crate) fn window_colors_change(
    _previous: Option<&ThemeName>,
    current: &ThemeName,
) -> Option<WindowColorsCmd> {
    Some(WindowColorsCmd::Apply(current.clone()))
}

fn reload_custom_rows(model: &mut Model, rows: Vec<CustomSetting>) {
    model.custom_rows = rows;
    let Model {
        workspace,
        custom_rows,
        ..
    } = model;
    let Some(Overlay::Settings(cursor)) = &mut workspace.overlay else {
        return;
    };
    *cursor = cursor.kept(&SettingRow::all(custom_rows));
}

fn music_dir_reloaded(
    music_dir: &mut std::path::PathBuf,
    reloaded: std::path::PathBuf,
) -> Cmd {
    if reloaded == *music_dir {
        Cmd::None
    } else {
        *music_dir = reloaded.clone();
        Effect::Library(LibraryCmd::Rescan {
            root: reloaded,
            revision: Revision::UNSTAMPED,
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
            ConfigFailure,
            KeyOverride,
            KeymapOverrides,
            Model,
            ThemeName,
            Themes,
            ToastLevel,
        },
        message::ConfigFact,
        update::config::config,
    };

    #[test]
    fn keymap_reload_bumps_the_config_generation() {
        let mut model = Model::default();
        let before = model.config_generation;

        let keys = KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]);
        let _ = config(&mut model, ConfigFact::KeymapReloaded(Box::new(keys))).unwrap();

        assert_ne!(model.config_generation, before);
    }

    #[test]
    fn unchanged_keymap_keeps_the_generation() {
        let mut model = Model::default();
        let keys = model.workspace.keymap.config().clone();
        let _ = config(
            &mut model,
            ConfigFact::KeymapReloaded(Box::new(keys.clone())),
        )
        .unwrap();
        let before = model.config_generation;

        let _ = config(&mut model, ConfigFact::KeymapReloaded(Box::new(keys))).unwrap();

        assert_eq!(model.config_generation, before);
    }

    #[test]
    fn theme_reload_bumps_the_theme_generation_and_cues() {
        let mut model = Model::default();
        let before = model.theme_generation;

        let cmd = config(
            &mut model,
            ConfigFact::ThemeReloaded(ThemeName::from_static("noir")),
        )
        .unwrap();

        assert_ne!(model.theme_generation, before);
        assert!(matches!(cmd, Cmd::Batch(_)));
    }

    #[test]
    fn watch_failure_raises_the_error_toast() {
        let mut model = Model::default();

        let _ = config(
            &mut model,
            ConfigFact::Failed(ConfigFailure::Watch {
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
            Cmd::One(Effect::Library(LibraryCmd::Rescan { root, .. })) => {
                Some(root.clone())
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

        let cmd =
            config(&mut model, ConfigFact::MusicDirReloaded(reloaded.clone())).unwrap();

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

        let cmd = config(
            &mut model,
            ConfigFact::ThemesLoaded(vec![ThemeName::from_static("wafer")]),
        )
        .unwrap();

        assert_eq!(model.themes.names, [ThemeName::from_static("wafer")]);
        assert!(matches!(cmd, Cmd::None));
    }

    #[rstest::rstest]
    #[case::first_reload_applies(None, "noir", Some("noir"))]
    #[case::same_name_again_applies(Some("noir"), "noir", Some("noir"))]
    #[case::other_name_applies(Some("noir"), "solar", Some("solar"))]
    fn window_colors_follow_a_theme_reload(
        #[case] previous: Option<&str>,
        #[case] current: &str,
        #[case] expected: Option<&str>,
    ) {
        let previous = previous.map(|name| ThemeName::new(name.to_string()).unwrap());
        let current = ThemeName::new(current.to_string()).unwrap();

        let applied =
            crate::update::config::window_colors_change(previous.as_ref(), &current);

        assert_eq!(
            applied.map(|command| match command {
                crate::cmd::WindowColorsCmd::Apply(name) => name.to_string(),
                crate::cmd::WindowColorsCmd::Reset => String::new(),
            }),
            expected.map(str::to_string)
        );
    }
}
