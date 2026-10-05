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
    cmd::{Cmd, Cmds, ConfigCmd},
    domain::{
        appearance::Appearance,
        config::{ConfigError, ConfigName},
        io_error::IoError,
        theme::{ThemeChoice, ThemeName},
    },
    message::{ConfigEvent, ConfigReload},
    update::machine::{LoopEffect, Machine, Unhandled},
};

use crate::{
    appearance_file::TomlAppearance,
    driver::{
        effect::{ConfigEffect, ConfigLoopCmd},
        message::ConfigMessage,
        paths::ConfigPaths,
        saves::PendingSaves,
        watch::{ConfigChange, ConfigWatch, ConfigWatchMessage, WatchEffect},
    },
    load::theme_parsed,
    theme_file::TomlTheme,
};

pub struct ConfigDriver<P: Fn(TomlTheme), A: Fn(Appearance)> {
    directory: Option<PathBuf>,
    config: PathBuf,
    appearance: PathBuf,
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
            .field("directory", &self.directory)
            .field("watch", &self.watch)
            .field("saves", &self.saves)
            .finish_non_exhaustive()
    }
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> ConfigDriver<P, A> {
    pub fn new(paths: &ConfigPaths, publish_theme: P, publish_appearance: A) -> Self {
        Self {
            directory: config_directory(&paths.appearance),
            config: paths.config.clone(),
            appearance: paths.appearance.clone(),
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
            ConfigMessage::Cmds(Cmds { cmds, .. }) => self.commanded(cmds),
            ConfigMessage::Started => {
                let watch = self.directory.clone().map_or_else(
                    || {
                        Cmd::message(ConfigEvent::Error(ConfigError::Watch(
                            IoError::Missing,
                        )))
                    },
                    |path| {
                        Cmd::effect(LoopEffect::Watch {
                            path,
                            item: ConfigMessage::Changed,
                        })
                    },
                );
                Ok(watch.then(self.poll_everything()))
            }
            ConfigMessage::Changed(Ok(())) => Ok(self.poll_everything()),
            ConfigMessage::Changed(Err(kind)) => {
                Ok(Cmd::message(ConfigEvent::Error(ConfigError::Watch(kind))))
            }
            ConfigMessage::Watch(message) => self.drive_watch(message),
            ConfigMessage::Elapsed(revision) => self.saves.elapsed(revision),
            ConfigMessage::Saved { file, text } => {
                self.drive_watch(ConfigWatchMessage::Saved { name: file, text })
            }
            ConfigMessage::Error(error) => Ok(Cmd::message(ConfigEvent::Error(error))),
        }
    }
}

impl<P: Fn(TomlTheme), A: Fn(Appearance)> ConfigDriver<P, A> {
    fn commanded(&mut self, cmds: Vec<ConfigCmd>) -> Result<ConfigLoopCmd, Unhandled> {
        let before = self.saves.revision();
        let (handled, cmd) = cmds
            .into_iter()
            .filter_map(|each| self.command(each).ok())
            .fold((0_usize, Cmd::none()), |(handled, all), next| {
                (handled + 1, all.then(next))
            });
        if handled == 0 {
            return Err(Unhandled);
        }
        Ok(cmd.then(self.saves.wait_since(before)))
    }

    fn command(&mut self, cmd: ConfigCmd) -> Result<ConfigLoopCmd, Unhandled> {
        match cmd {
            ConfigCmd::Save(patch) => {
                self.saves.hold_config(patch);
                Ok(Cmd::none())
            }
            ConfigCmd::SelectTheme(ThemeChoice::Named(name)) => {
                self.drive_watch(ConfigWatchMessage::SelectTheme(name))
            }
            ConfigCmd::SelectTheme(ThemeChoice::Auto) => Err(Unhandled),
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
        let lifted: ConfigLoopCmd = effects.into_iter().map(lift).collect();
        let default_music_dir = self.default_music_dir.as_deref();
        Ok(changes
            .into_iter()
            .map(|change| changed(change, default_music_dir))
            .fold(lifted, Cmd::then))
    }

    fn poll_everything(&mut self) -> ConfigLoopCmd {
        [
            ConfigWatchMessage::PollAppearance,
            ConfigWatchMessage::PollConfig,
            ConfigWatchMessage::PollTheme,
            ConfigWatchMessage::PollThemes,
        ]
        .into_iter()
        .map(|message| {
            self.drive_watch(message)
                .unwrap_or_else(|Unhandled| Cmd::none())
        })
        .fold(Cmd::none(), Cmd::then)
    }
}

fn changed(change: ConfigChange, default_music_dir: Option<&Path>) -> ConfigLoopCmd {
    match change {
        ConfigChange::Appearance(text) => appearance_changed(text.as_deref()),
        ConfigChange::Keymap(text) => {
            keymap_changed(text.as_deref(), default_music_dir)
        }
        ConfigChange::Theme { name, text } => theme_changed(name, text.as_deref()),
        ConfigChange::Themes(names) => {
            Cmd::message(ConfigEvent::ThemesLoaded(embedded_and_user(names)))
        }
    }
}

fn appearance_changed(text: Option<&str>) -> ConfigLoopCmd {
    let parsed = text.map_or_else(
        || Ok(TomlAppearance::default()),
        crate::appearance_file::parse_appearance,
    );
    match parsed {
        Ok(file) => {
            let settings = file.settings();
            Cmd::effect(LoopEffect::Execute(ConfigEffect::PublishAppearance(
                file.appearance(),
            )))
            .then(reports([
                ConfigEvent::AppearanceReloaded(settings),
                reloaded(ConfigName::Appearance, Ok(())),
            ]))
        }
        Err(error) => Cmd::message(reloaded(
            ConfigName::Appearance,
            Err(ConfigError::invalid(&error)),
        )),
    }
}

fn keymap_changed(
    text: Option<&str>,
    default_music_dir: Option<&Path>,
) -> ConfigLoopCmd {
    match crate::config_file::parse_config_reload(text.unwrap_or("")) {
        Ok(parsed) => {
            let keymap = ConfigEvent::KeymapReloaded(Box::new(parsed.keymap));
            let music_dir = parsed
                .music_dir
                .or_else(|| default_music_dir.map(Path::to_path_buf))
                .map(ConfigEvent::MusicDirReloaded);
            reports(
                std::iter::once(keymap)
                    .chain(music_dir)
                    .chain([reloaded(ConfigName::Config, Ok(()))]),
            )
        }
        Err(error) => Cmd::message(reloaded(
            ConfigName::Config,
            Err(ConfigError::invalid(&error)),
        )),
    }
}

fn lift(effect: WatchEffect) -> LoopEffect<ConfigEffect, Infallible, ConfigMessage> {
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

fn embedded_and_user(user: Vec<ThemeName>) -> Vec<ThemeName> {
    crate::embedded_theme::EMBEDDED_THEMES
        .iter()
        .map(|&(name, _)| ThemeName::from_static(name))
        .chain(user)
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

fn config_directory(appearance: &Path) -> Option<PathBuf> {
    appearance
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
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
            saves::SAVE_DEBOUNCE,
            watch::{ConfigWatchMessage, WatchEffect},
        },
        theme_file::TomlTheme,
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbackground = \"#000000\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    const KEYS_X: &str = "[keymap]\nnext = \"x\"\n";
    const KEYS_Y: &str = "[keymap]\nnext = \"y\"\n";

    type Driver = ConfigDriver<fn(TomlTheme), fn(Appearance)>;

    fn seeded(theme: Option<&'static str>, seen: SeenTexts) -> Driver {
        let paths = ConfigPaths {
            config: PathBuf::from("/config/config.toml"),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            default_music_dir: None,
            theme: theme.map(ThemeName::from_static),
            seen,
        };
        ConfigDriver::new(&paths, drop::<TomlTheme>, drop::<Appearance>)
    }

    fn driver(theme: Option<&'static str>) -> Driver {
        seeded(theme, SeenTexts::default())
    }

    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    fn commanded(cmds: Vec<ConfigCmd>) -> ConfigMessage {
        ConfigMessage::Cmds(Cmds {
            cmds,
            at: Instant::now(),
        })
    }

    fn saving(patch: ConfigPatch) -> ConfigMessage {
        commanded(vec![ConfigCmd::Save(patch)])
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

    fn after(revision: u64) -> Cmd<String, ConfigEvent> {
        Cmd::effect(describe(LoopEffect::After {
            delay: SAVE_DEBOUNCE,
            message: ConfigMessage::Elapsed(
                (0..revision).fold(Revision::default(), |at, _| at.next()),
            ),
        }))
    }

    fn reading(file: ConfigName, path: &str) -> ConfigEffect {
        ConfigEffect::Watch(WatchEffect::Read {
            file,
            path: PathBuf::from(path),
        })
    }

    fn cover_brackets() -> AppearancePatch {
        AppearancePatch::builder()
            .cover_brackets(CoverBrackets::Shown)
            .build()
    }

    fn setting() -> ConfigMessage {
        commanded(vec![ConfigCmd::SetAppearance(cover_brackets())])
    }

    fn appearance_read(mut driver: Driver) -> Driver {
        let read = driver.transition(read_done(ConfigName::Appearance, None));
        assert!(read.is_ok());
        driver
    }

    fn step(driver: &mut Driver, message: ConfigMessage) -> ConfigLoopCmd {
        driver.transition(message).unwrap()
    }

    #[rstest]
    #[case::started(ConfigMessage::Started, Ok(Cmd::from_iter([
        "watch /config".to_string(),
        executed(reading(ConfigName::Appearance, "/config/sifr-ui.toml")),
        executed(reading(ConfigName::Config, "/config/config.toml")),
        executed(ConfigEffect::Watch(WatchEffect::List(PathBuf::from(
            "/config/themes"
        )))),
    ])))]
    #[case::absent_config_stays_absent(
        read_done(ConfigName::Config, None),
        Ok(Cmd::none())
    )]
    #[case::own_appearance_write(ConfigMessage::Saved { file: ConfigName::Appearance, text: "[window]\n".to_string() }, Ok(Cmd::none()))]
    #[case::save_error(
        ConfigMessage::Error(ConfigError::Save { file: ConfigName::Config, kind: IoError::Other }),
        Ok(Cmd::message(ConfigEvent::Error(ConfigError::Save { file: ConfigName::Config, kind: IoError::Other }))),
    )]
    #[case::watch_error(
        ConfigMessage::Changed(Err(IoError::Missing)),
        Ok(Cmd::message(ConfigEvent::Error(ConfigError::Watch(IoError::Missing))))
    )]
    #[case::select_theme(commanded(vec![ConfigCmd::SelectTheme("noir".parse().unwrap())]), Ok(Cmd::effect(executed(reading(noir(), "/config/themes/noir.toml")))))]
    #[case::save(saving(ConfigPatch::builder().build()), Ok(after(1)))]
    fn a_fresh_driver_answers(
        #[case] message: ConfigMessage,
        #[case] expected: Result<Cmd<String, ConfigEvent>, Unhandled>,
    ) {
        let mut fresh = driver(None);

        assert_eq!(fresh.transition(message).map(described), expected);
    }

    #[rstest]
    #[case::nothing_pending(ConfigMessage::Elapsed(Revision::default()))]
    #[case::auto_theme(commanded(vec![ConfigCmd::SelectTheme(ThemeChoice::Auto)]))]
    fn a_fresh_driver_refuses_and_stays_unchanged(#[case] message: ConfigMessage) {
        let mut fresh = driver(None);
        let before = format!("{fresh:?}");

        assert!(matches!(fresh.transition(message), Err(Unhandled)));
        assert_eq!(format!("{fresh:?}"), before);
    }

    #[test]
    fn the_first_external_config_edit_reloads_and_clears_the_error() {
        let seen = SeenTexts {
            config: Some(KEYS_X.to_string()),
            ..SeenTexts::default()
        };
        let mut settled = seeded(None, seen);
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
    fn a_changed_theme_publishes_then_tells_theme_reloaded() {
        let mut state = driver(Some("noir"));

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

        let (effects, _) =
            step(&mut state, read_done(noir(), Some(NOIR_THEME))).into_parts();

        assert!(matches!(
            effects.as_slice(),
            [LoopEffect::Execute(ConfigEffect::PublishTheme(_))]
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
                result: Err(ConfigError::Invalid(_)),
            })]
        ));
    }

    #[test]
    fn a_burst_of_saves_becomes_one_write_carrying_every_field() {
        let mut current = driver(None);
        let first = step(
            &mut current,
            saving(
                ConfigPatch::builder()
                    .theme(ThemeName::from_static("noir"))
                    .build(),
            ),
        );
        assert_eq!(described(first), after(1));
        for volume in [10_u8, 20, 30, 40, 50] {
            let patch = ConfigPatch::builder()
                .volume(Percent::clamped(volume))
                .build();
            let queued = step(&mut current, saving(patch));
            assert_eq!(
                described(queued),
                after(u64::from(volume / 10) + 1),
                "every save pushes the trailing edge out"
            );
        }

        let before_stale = format!("{current:?}");
        let stale =
            current.transition(ConfigMessage::Elapsed(Revision::default().next()));
        assert_eq!(format!("{current:?}"), before_stale);
        let elapsed = ConfigMessage::Elapsed(current.saves.revision());
        let due = step(&mut current, elapsed);

        assert!(matches!(stale, Err(Unhandled)));
        let expected = ConfigPatch::builder()
            .theme(ThemeName::from_static("noir"))
            .volume(Percent::clamped(50))
            .build();
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
        assert_eq!(described(Cmd::from_iter(effects)), after(1));
    }

    #[test]
    fn one_batch_saves_both_files_together() {
        let mut next = appearance_read(driver(None));
        let both = commanded(vec![
            ConfigCmd::Save(ConfigPatch::builder().build()),
            ConfigCmd::SetAppearance(cover_brackets()),
        ]);

        let (effects, _) = step(&mut next, both).into_parts();
        let elapsed = ConfigMessage::Elapsed(next.saves.revision());
        let (due, _) = step(&mut next, elapsed).into_parts();

        assert_eq!(described(Cmd::from_iter(effects)), after(2));
        assert!(matches!(
            due.as_slice(),
            [
                LoopEffect::Execute(ConfigEffect::SaveConfig(_)),
                LoopEffect::Execute(ConfigEffect::SaveAppearance(_)),
            ]
        ));
    }

    #[rstest]
    #[case::config(vec![ConfigCmd::Save(ConfigPatch::builder().build())], &["save_config"])]
    #[case::appearance(vec![ConfigCmd::SetAppearance(cover_brackets())], &["save_appearance"])]
    #[case::both(vec![ConfigCmd::SetAppearance(cover_brackets()), ConfigCmd::Save(ConfigPatch::builder().build())], &["save_config", "save_appearance"])]
    #[case::nothing(vec![], &[])]
    fn flush_writes_every_pending_save_at_once(
        #[case] pending: Vec<ConfigCmd>,
        #[case] expected: &[&str],
    ) {
        let mut next = appearance_read(driver(None));
        if !pending.is_empty() {
            assert!(next.transition(commanded(pending)).is_ok());
        }
        let old = next.saves.revision();

        let (flushed, _) =
            step(&mut next, commanded(vec![ConfigCmd::Flush])).into_parts();
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
        let flushed_state = format!("{next:?}");
        assert!(matches!(
            next.transition(ConfigMessage::Elapsed(old)),
            Err(Unhandled)
        ));
        assert_eq!(format!("{next:?}"), flushed_state);
    }
}
