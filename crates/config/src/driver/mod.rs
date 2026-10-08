pub mod effect;
mod execute;
pub(crate) mod files;
pub mod message;
pub mod paths;
mod saves;
pub mod watch;

use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};

use kernel::{
    cmd::{Cmd, ConfigCmd},
    domain::{
        appearance::Appearance,
        config::{ConfigError, ConfigName, Diagnostic},
        io_error::IoError,
        theme::ThemeName,
    },
    message::{ConfigEvent, ConfigReload},
    update::machine::{LoopEffect, Machine, Unhandled, each_handled},
};

use crate::{
    appearance_file::parse_appearance,
    config_file::parse_config_settings,
    driver::{
        effect::{ConfigEffect, ConfigLoopCmd},
        files::parent_dir,
        message::ConfigMessage,
        paths::ConfigPaths,
        saves::PendingSaves,
        watch::{ConfigChange, ConfigWatch, ConfigWatchEffect, ConfigWatchMessage},
    },
    embedded_theme::{EMBEDDED_THEMES, theme_name},
    load::theme_parsed,
    theme_file::TomlTheme,
};

pub struct ConfigDriver<P: Fn(TomlTheme), A: Fn(Appearance)> {
    dir: Option<PathBuf>,
    default_music_dir: Option<PathBuf>,
    watch: ConfigWatch,
    saves: PendingSaves,
    publish_theme: P,
    publish_appearance: A,
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> std::fmt::Debug for ConfigDriver<P, A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigDriver")
            .field("directory", &self.dir)
            .field("watch", &self.watch)
            .field("saves", &self.saves)
            .finish_non_exhaustive()
    }
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> ConfigDriver<P, A> {
    pub fn new(paths: &ConfigPaths, publish_theme: P, publish_appearance: A) -> Self {
        Self {
            dir: parent_dir(&paths.appearance_path).map(Path::to_path_buf),
            default_music_dir: paths.default_music_dir.clone(),
            watch: ConfigWatch::new(paths),
            saves: PendingSaves::default(),
            publish_theme,
            publish_appearance,
        }
    }
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> Machine for ConfigDriver<P, A> {
    type Message = ConfigMessage;
    type Effect = ConfigLoopCmd;

    fn transition(
        &mut self,
        message: ConfigMessage,
    ) -> Result<ConfigLoopCmd, Unhandled> {
        match message {
            ConfigMessage::Cmds(batch) => self.transition_cmds(batch.cmds),
            ConfigMessage::Started => {
                let watch = self.dir.clone().map_or_else(
                    || {
                        Cmd::message(ConfigEvent::Error(ConfigError::Watch(
                            IoError::Missing,
                        )))
                    },
                    |path| {
                        Cmd::effect(LoopEffect::Watch {
                            path,
                            changed: ConfigMessage::Changed,
                        })
                    },
                );
                Ok(watch.then(self.poll_everything()?))
            }
            ConfigMessage::Changed(Ok(())) => self.poll_everything(),
            ConfigMessage::Changed(Err(error)) => {
                Ok(Cmd::message(ConfigEvent::Error(ConfigError::Watch(error))))
            }
            ConfigMessage::Watch(message) => self.drive_watch(message),
            ConfigMessage::Elapsed(revision) => self.saves.elapsed(revision),
            ConfigMessage::Error(error) => Ok(Cmd::message(ConfigEvent::Error(error))),
        }
    }
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> ConfigDriver<P, A> {
    fn transition_cmds(
        &mut self,
        config_cmds: Vec<ConfigCmd>,
    ) -> Result<ConfigLoopCmd, Unhandled> {
        let before = self.saves.revision();
        each_handled(config_cmds, |each| self.transition_cmd(each))
            .map(|cmd| cmd.then(self.saves.wait_since(before)))
    }

    fn transition_cmd(
        &mut self,
        config_cmd: ConfigCmd,
    ) -> Result<ConfigLoopCmd, Unhandled> {
        match config_cmd {
            ConfigCmd::Save(patch) => {
                self.saves.hold_config(patch);
                Ok(Cmd::none())
            }
            ConfigCmd::SelectTheme(theme_choice) => self.drive_watch(
                ConfigWatchMessage::SelectTheme(theme_name(&theme_choice)),
            ),
            ConfigCmd::SetAppearance(patch) => {
                self.saves.hold_appearance(patch);
                Ok(Cmd::none())
            }
            ConfigCmd::Flush => Ok(self.saves.flush()),
        }
    }

    fn drive_watch(
        &mut self,
        message: ConfigWatchMessage,
    ) -> Result<ConfigLoopCmd, Unhandled> {
        let (effects, changes) = self.watch.transition(message)?.into_parts();
        let config_loop_cmd: ConfigLoopCmd = effects.into_iter().map(lift).collect();
        let default_music_dir = self.default_music_dir.as_deref();
        Ok(changes
            .into_iter()
            .map(|change| changed(change, default_music_dir))
            .fold(config_loop_cmd, Cmd::then))
    }

    fn poll_everything(&mut self) -> Result<ConfigLoopCmd, Unhandled> {
        each_handled(
            vec![
                ConfigWatchMessage::PollAppearance,
                ConfigWatchMessage::PollConfig,
                ConfigWatchMessage::PollTheme,
                ConfigWatchMessage::PollThemes,
            ],
            |message| self.drive_watch(message),
        )
    }
}

fn changed(change: ConfigChange, default_music_dir: Option<&Path>) -> ConfigLoopCmd {
    match change {
        ConfigChange::Appearance(text) => appearance_changed(text.as_deref()),
        ConfigChange::Config(text) => {
            config_changed(text.as_deref(), default_music_dir)
        }
        ConfigChange::Theme { name, text } => theme_changed(name, text.as_deref()),
        ConfigChange::Themes {
            theme_names,
            refused,
        } => Cmd::message(ConfigEvent::ThemesLoaded {
            theme_names: embedded_and_user(theme_names),
            refused,
        }),
    }
}

fn appearance_changed(text: Option<&str>) -> ConfigLoopCmd {
    match parse_appearance(text.unwrap_or("")) {
        Ok(file) => {
            let appearance_settings = file.to_appearance_settings();
            Cmd::effect(LoopEffect::Execute(ConfigEffect::PublishAppearance(
                file.to_appearance(),
            )))
            .then(reports([
                ConfigEvent::AppearanceReloaded(appearance_settings),
                reloaded(ConfigName::Appearance, Ok(())),
            ]))
        }
        Err(error) => Cmd::message(reloaded(
            ConfigName::Appearance,
            Err(Diagnostic::from_error(&error).into()),
        )),
    }
}

fn config_changed(
    text: Option<&str>,
    default_music_dir: Option<&Path>,
) -> ConfigLoopCmd {
    match parse_config_settings(text.unwrap_or("")) {
        Ok(parsed) => {
            let keymap_event =
                ConfigEvent::KeymapReloaded(Box::new(parsed.keymap_overrides));
            let music_dir = parsed
                .music_dir
                .or_else(|| default_music_dir.map(Path::to_path_buf))
                .map(ConfigEvent::MusicDirReloaded);
            reports(
                std::iter::once(keymap_event)
                    .chain(music_dir)
                    .chain([reloaded(ConfigName::Config, Ok(()))]),
            )
        }
        Err(error) => Cmd::message(reloaded(
            ConfigName::Config,
            Err(Diagnostic::from_error(&error).into()),
        )),
    }
}

fn lift(
    effect: ConfigWatchEffect,
) -> LoopEffect<ConfigEffect, Infallible, ConfigMessage> {
    LoopEffect::Execute(ConfigEffect::Watch(effect))
}

fn theme_changed(name: ThemeName, text: Option<&str>) -> ConfigLoopCmd {
    match theme_parsed(&name, text) {
        Ok(file) => Cmd::effect(LoopEffect::Execute(ConfigEffect::PublishTheme(file)))
            .then(reports([
                ConfigEvent::ThemeReloaded(name.clone()),
                reloaded(ConfigName::Theme(name), Ok(())),
            ])),
        Err(error) => Cmd::message(reloaded(ConfigName::Theme(name), Err(error))),
    }
}

fn reloaded(name: ConfigName, result: Result<(), ConfigError>) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload { name, result })
}

fn reports(events: impl IntoIterator<Item = ConfigEvent>) -> ConfigLoopCmd {
    events
        .into_iter()
        .map(Cmd::message)
        .fold(Cmd::none(), Cmd::then)
}

fn embedded_and_user(user_theme_names: Vec<ThemeName>) -> Vec<ThemeName> {
    EMBEDDED_THEMES
        .iter()
        .map(|&(name, _)| ThemeName::from_static(name))
        .chain(user_theme_names)
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf, time::Instant};

    use kernel::{
        cmd::{Cmd, Cmds, ConfigCmd, ConfigPatch},
        domain::{
            appearance::{AppearancePatch, CoverBrackets},
            bounded::Bounded,
            config::{ConfigError, ConfigName},
            io_error::IoError,
            percent::Percent,
            revision::Revision,
            theme::{ThemeChoice, ThemeName},
        },
        message::{ConfigEvent, ConfigReload},
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        driver::{
            Appearance,
            ConfigDriver,
            effect::{ConfigEffect, ConfigLoopCmd},
            message::ConfigMessage,
            paths::{ConfigPaths, SeenTexts},
            saves::{PendingSaves, SAVE_DEBOUNCE},
            watch::{ConfigWatch, ConfigWatchEffect, ConfigWatchMessage, SavedFile},
        },
        embedded_theme::EMBEDDED_THEMES,
        theme_file::TomlTheme,
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbackground = \"#000000\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    const KEYS_X: &str = "[keymap]\nnext = \"x\"\n";
    const KEYS_Y: &str = "[keymap]\nnext = \"y\"\n";

    type Driver = ConfigDriver<fn(TomlTheme), fn(Appearance)>;

    fn seeded(theme: Option<&'static str>, seen_texts: SeenTexts) -> Driver {
        let paths = ConfigPaths {
            config_path: PathBuf::from("/config/config.toml"),
            appearance_path: PathBuf::from("/config/sifr-ui.toml"),
            themes_dir: PathBuf::from("/config/themes"),
            default_music_dir: None,
            theme_name: theme.map(ThemeName::from_static),
            seen_texts,
        };
        ConfigDriver::new(&paths, drop::<TomlTheme>, drop::<Appearance>)
    }

    fn driver(theme: Option<&'static str>) -> Driver {
        seeded(theme, SeenTexts::default())
    }

    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    fn cmds(config_cmds: Vec<ConfigCmd>) -> ConfigMessage {
        ConfigMessage::Cmds(Cmds {
            cmds: config_cmds,
            at: Instant::now(),
        })
    }

    fn saving(patch: ConfigPatch) -> ConfigMessage {
        cmds(vec![ConfigCmd::Save(patch)])
    }

    fn read_done(name: ConfigName, text: Option<&str>) -> ConfigMessage {
        ConfigMessage::Watch(ConfigWatchMessage::ReadDone {
            name,
            text: text.map(str::to_string),
        })
    }

    fn describe(effect: LoopEffect<ConfigEffect, Infallible, ConfigMessage>) -> String {
        match effect {
            LoopEffect::Execute(effect) => format!("{effect:?}"),
            LoopEffect::Run(job) => match job {},
            LoopEffect::After { delay, message } => {
                format!("after {delay:?} {message:?}")
            }
            LoopEffect::Watch { path, .. } => format!("watch {}", path.display()),
            LoopEffect::Unwatch(path) => format!("unwatch {}", path.display()),
        }
    }

    fn described(loop_cmd: ConfigLoopCmd) -> Cmd<String, ConfigEvent> {
        loop_cmd.map_effect(describe)
    }

    fn executed(effect: ConfigEffect) -> String {
        describe(LoopEffect::Execute(effect))
    }

    fn issued(count: u64) -> Revision {
        (0..count).fold(Revision::default(), |at, _| at.next())
    }

    fn after(revision: Revision) -> Cmd<String, ConfigEvent> {
        Cmd::effect(describe(LoopEffect::After {
            delay: SAVE_DEBOUNCE,
            message: ConfigMessage::Elapsed(revision),
        }))
    }

    fn reading(config_name: ConfigName, path: &str) -> ConfigEffect {
        ConfigEffect::Watch(ConfigWatchEffect::Read {
            name: config_name,
            path: PathBuf::from(path),
        })
    }

    fn cover_brackets() -> AppearancePatch {
        AppearancePatch {
            cover_brackets: Some(CoverBrackets::Shown),
            ..AppearancePatch::default()
        }
    }

    fn setting() -> ConfigMessage {
        cmds(vec![ConfigCmd::SetAppearance(cover_brackets())])
    }

    fn appearance_read(mut driver: Driver) -> Driver {
        let read = driver.transition(read_done(ConfigName::Appearance, None));
        assert!(read.is_ok());
        driver
    }

    fn step(driver: &mut Driver, message: ConfigMessage) -> ConfigLoopCmd {
        driver.transition(message).unwrap()
    }

    fn state(
        driver: &Driver,
    ) -> (Option<PathBuf>, Option<PathBuf>, ConfigWatch, PendingSaves) {
        (
            driver.dir.clone(),
            driver.default_music_dir.clone(),
            driver.watch.clone(),
            driver.saves.clone(),
        )
    }

    #[rstest]
    #[case::started(ConfigMessage::Started, Ok(Cmd::from_iter([
        "watch /config".to_string(),
        executed(reading(ConfigName::Appearance, "/config/sifr-ui.toml")),
        executed(reading(ConfigName::Config, "/config/config.toml")),
        executed(ConfigEffect::Watch(ConfigWatchEffect::List(PathBuf::from(
            "/config/themes"
        )))),
    ])))]
    #[case::absent_config_stays_absent(
        read_done(ConfigName::Config, None),
        Ok(Cmd::none())
    )]
    #[case::own_appearance_write(ConfigMessage::Watch(ConfigWatchMessage::Saved { saved_file: SavedFile::Appearance, text: "[window]\n".to_string() }), Ok(Cmd::none()))]
    #[case::save_error(
        ConfigMessage::Error(ConfigError::Save { name: ConfigName::Config, error: IoError::Other }),
        Ok(Cmd::message(ConfigEvent::Error(ConfigError::Save { name: ConfigName::Config, error: IoError::Other }))),
    )]
    #[case::watch_error(
        ConfigMessage::Changed(Err(IoError::Missing)),
        Ok(Cmd::message(ConfigEvent::Error(ConfigError::Watch(IoError::Missing))))
    )]
    #[case::select_theme(cmds(vec![ConfigCmd::SelectTheme("noir".parse().unwrap())]), Ok(Cmd::effect(executed(reading(noir(), "/config/themes/noir.toml")))))]
    #[case::auto_theme(cmds(vec![ConfigCmd::SelectTheme(ThemeChoice::Auto)]), Ok(Cmd::effect(executed(reading(noir(), "/config/themes/noir.toml")))))]
    #[case::save(saving(ConfigPatch::default()), Ok(after(issued(1))))]
    #[case::themes_listed(
        ConfigMessage::Watch(ConfigWatchMessage::Listed { theme_names: vec![ThemeName::from_static("noir"), ThemeName::from_static("mine")], refused: vec!["auto".to_string()] }),
        Ok(Cmd::message(ConfigEvent::ThemesLoaded {
            theme_names: EMBEDDED_THEMES.iter().map(|&(name, _)| ThemeName::from_static(name)).chain([ThemeName::from_static("mine")]).collect(),
            refused: vec!["auto".to_string()],
        }))
    )]
    fn a_fresh_driver_answers(
        #[case] message: ConfigMessage,
        #[case] expected: Result<Cmd<String, ConfigEvent>, Unhandled>,
    ) {
        let mut fresh = driver(None);

        assert_eq!(fresh.transition(message).map(described), expected);
    }

    #[test]
    fn the_first_external_config_edit_reloads_and_clears_the_error() {
        let seen_texts = SeenTexts {
            config: Some(KEYS_X.to_string()),
            ..SeenTexts::default()
        };
        let mut settled = seeded(None, seen_texts);
        settled.default_music_dir = Some(PathBuf::from("/music"));
        let startup_read =
            step(&mut settled, read_done(ConfigName::Config, Some(KEYS_X)));
        assert_eq!(described(startup_read), Cmd::none());

        let (effects, events) =
            step(&mut settled, read_done(ConfigName::Config, Some(KEYS_Y)))
                .into_parts();

        assert!(effects.is_empty());
        let default_dir = Some(ConfigEvent::MusicDirReloaded(PathBuf::from("/music")));
        assert!(matches!(
            events.first(),
            Some(ConfigEvent::KeymapReloaded(_))
        ));
        assert_eq!(
            events.get(1..events.len() - 1),
            Some(Vec::from_iter(default_dir).as_slice()),
            "a removed music_dir falls back to the default"
        );
        assert!(matches!(
            events.last(),
            Some(ConfigEvent::Reloaded(ConfigReload {
                name: ConfigName::Config,
                result: Ok(())
            }))
        ));
    }

    #[test]
    fn started_publishes_the_current_theme() {
        let mut state = driver(Some("noir"));
        let started = step(&mut state, ConfigMessage::Started);
        let (started_effects, _) = started.into_parts();
        assert!(
            started_effects
                .into_iter()
                .map(describe)
                .any(|each| each
                    == executed(reading(noir(), "/config/themes/noir.toml")))
        );

        let (effects, events) =
            step(&mut state, read_done(noir(), Some(NOIR_THEME))).into_parts();

        assert!(matches!(
            effects.as_slice(),
            [LoopEffect::Execute(ConfigEffect::PublishTheme(_))]
        ));
        assert!(matches!(
            events.as_slice(),
            [
                ConfigEvent::ThemeReloaded(_),
                ConfigEvent::Reloaded(ConfigReload {
                    name: ConfigName::Theme(_),
                    result: Ok(())
                }),
            ]
        ));
    }

    #[test]
    fn a_changed_appearance_publishes_then_tells_appearance_reloaded() {
        let mut state = driver(None);

        let (effects, events) = step(
            &mut state,
            read_done(ConfigName::Appearance, Some("[cover]\nmode = \"plain\"\n")),
        )
        .into_parts();

        assert!(matches!(
            effects.as_slice(),
            [LoopEffect::Execute(ConfigEffect::PublishAppearance(_))]
        ));
        assert!(matches!(
            events.as_slice(),
            [ConfigEvent::AppearanceReloaded(_), ..]
        ));
    }

    #[test]
    fn an_unparsable_appearance_tells_config_failed() {
        let mut state = driver(None);

        let (effects, events) = step(
            &mut state,
            read_done(ConfigName::Appearance, Some("[cover\nnot toml")),
        )
        .into_parts();

        assert!(effects.is_empty());
        assert!(matches!(
            events.as_slice(),
            [ConfigEvent::Reloaded(ConfigReload {
                name: ConfigName::Appearance,
                result: Err(ConfigError::Parse(_)),
            })]
        ));
    }

    #[test]
    fn a_burst_of_saves_becomes_one_write_carrying_every_field() {
        let mut current = driver(None);
        let first = step(
            &mut current,
            saving(ConfigPatch {
                theme_name: Some(ThemeName::from_static("noir")),
                ..ConfigPatch::default()
            }),
        );
        assert_eq!(described(first), after(issued(1)));
        for volume in [10_u8, 20, 30, 40, 50] {
            let patch = ConfigPatch {
                volume: Some(Percent::clamped(volume)),
                ..ConfigPatch::default()
            };
            let queued = step(&mut current, saving(patch));
            assert_eq!(
                described(queued),
                after(issued(u64::from(volume / 10) + 1)),
                "every save pushes the trailing edge out"
            );
        }

        let before_stale = state(&current);
        let stale =
            current.transition(ConfigMessage::Elapsed(Revision::default().next()));
        assert_eq!(state(&current), before_stale);
        let config_message = ConfigMessage::Elapsed(current.saves.revision());
        let due = step(&mut current, config_message);

        assert!(matches!(stale, Err(Unhandled)));
        let expected = ConfigPatch {
            theme_name: Some(ThemeName::from_static("noir")),
            volume: Some(Percent::clamped(50)),
            ..ConfigPatch::default()
        };
        assert_eq!(
            described(due),
            Cmd::effect(executed(ConfigEffect::SaveConfig(expected)))
        );
    }

    #[test]
    fn a_setting_queues_a_save_and_tells_nothing_back() {
        let mut next = appearance_read(driver(None));

        let (effects, events) = step(&mut next, setting()).into_parts();

        assert!(events.is_empty());
        assert_eq!(described(Cmd::from_iter(effects)), after(issued(1)));
    }

    #[test]
    fn one_batch_saves_both_files_together() {
        let mut next = appearance_read(driver(None));
        let both = cmds(vec![
            ConfigCmd::Save(ConfigPatch::default()),
            ConfigCmd::SetAppearance(cover_brackets()),
        ]);

        let (effects, _) = step(&mut next, both).into_parts();
        let config_message = ConfigMessage::Elapsed(next.saves.revision());
        let (due, _) = step(&mut next, config_message).into_parts();

        assert_eq!(described(Cmd::from_iter(effects)), after(issued(2)));
        assert!(matches!(
            due.as_slice(),
            [
                LoopEffect::Execute(ConfigEffect::SaveConfig(_)),
                LoopEffect::Execute(ConfigEffect::SaveAppearance(_)),
            ]
        ));
    }

    #[rstest]
    #[case::config(vec![ConfigCmd::Save(ConfigPatch::default())], &["save_config"])]
    #[case::appearance(vec![ConfigCmd::SetAppearance(cover_brackets())], &["save_appearance"])]
    #[case::both(vec![ConfigCmd::SetAppearance(cover_brackets()), ConfigCmd::Save(ConfigPatch::default())], &["save_config", "save_appearance"])]
    #[case::nothing(vec![], &[])]
    fn flush_writes_every_pending_save_at_once(
        #[case] pending_config_cmds: Vec<ConfigCmd>,
        #[case] expected: &[&str],
    ) {
        let mut next = appearance_read(driver(None));
        if !pending_config_cmds.is_empty() {
            assert!(next.transition(cmds(pending_config_cmds)).is_ok());
        }
        let old = next.saves.revision();

        let (flushed, _) = step(&mut next, cmds(vec![ConfigCmd::Flush])).into_parts();
        let names: Vec<&str> = flushed
            .iter()
            .map(|each| {
                if matches!(each, LoopEffect::Execute(ConfigEffect::SaveConfig(_))) {
                    "save_config"
                } else if matches!(
                    each,
                    LoopEffect::Execute(ConfigEffect::SaveAppearance(_))
                ) {
                    "save_appearance"
                } else {
                    "other"
                }
            })
            .collect();

        assert_eq!(names, expected);
        let flushed_state = state(&next);
        assert!(matches!(
            next.transition(ConfigMessage::Elapsed(old)),
            Err(Unhandled)
        ));
        assert_eq!(state(&next), flushed_state);
    }
}
