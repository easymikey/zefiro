mod disk;
mod saves;
mod watch;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use kernel::{
    Cmd,
    Cmds,
    ConfigCmd,
    ConfigEvent,
    ConfigPatch,
    ConfigReload,
    domain::{
        ConfigError,
        ConfigName,
        OptionIndex,
        Revision,
        ThemeName,
        appearance_rows::{AppearanceField, appearance_patch, appearance_settings},
    },
    update::{Machine, Unhandled},
};
use strum::IntoStaticStr;
pub use watch::ConfigChange;

use crate::{
    AppearanceFile,
    AppearancePatch,
    ThemeFile,
    driver::{
        saves::SaveQueue,
        watch::{ConfigWatch, WatchEffect, WatchMessage},
    },
};

#[must_use]
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub config: PathBuf,
    pub appearance: PathBuf,
    pub themes: PathBuf,
    pub theme: Option<String>,
    pub seen: SeenTexts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenTexts {
    pub appearance: Option<String>,
    pub theme: Option<String>,
    pub config: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Sighting {
    #[default]
    First,
    Repeat,
}

pub struct ConfigDriver<P: Fn(ThemeFile)> {
    directory: PathBuf,
    paths: ConfigPaths,
    watch: ConfigWatch,
    config: Sighting,
    appearance: Sighting,
    appearance_file: AppearanceFile,
    saves: SaveQueue,
    publish: P,
}

#[derive(Debug, PartialEq, IntoStaticStr)]
pub enum ConfigMessage {
    Cmds(Cmds<ConfigCmd>),
    Started,
    FilesChanged,
    ReadDone {
        file: ConfigName,
        text: Option<String>,
    },
    Listed(Vec<String>),
    Elapsed(Revision),
    Saved {
        file: ConfigName,
        text: String,
    },
    Changed(ConfigChange),
    Error(ConfigError),
}

impl From<Cmds<ConfigCmd>> for ConfigMessage {
    fn from(cmds: Cmds<ConfigCmd>) -> Self {
        ConfigMessage::Cmds(cmds)
    }
}

#[derive(Debug, PartialEq)]
pub enum ConfigEffect {
    Watch(PathBuf),
    Read { file: ConfigName, path: PathBuf },
    List(PathBuf),
    After { delay: Duration, revision: Revision },
    SaveConfig(ConfigPatch),
    SaveAppearance(AppearancePatch),
    Publish(ThemeFile),
}

impl<P: Fn(ThemeFile)> std::fmt::Debug for ConfigDriver<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigDriver")
            .field("directory", &self.directory)
            .field("watch", &self.watch)
            .field("saves", &self.saves)
            .finish_non_exhaustive()
    }
}

impl<P: Fn(ThemeFile)> ConfigDriver<P> {
    pub fn new(paths: &ConfigPaths, publish: P) -> Self {
        Self {
            directory: config_directory(&paths.appearance),
            paths: ConfigPaths {
                config: paths.config.clone(),
                appearance: paths.appearance.clone(),
                themes: paths.themes.clone(),
                theme: paths.theme.clone(),
                seen: SeenTexts::default(),
            },
            watch: ConfigWatch::new(paths),
            config: Sighting::First,
            appearance: Sighting::First,
            appearance_file: AppearanceFile::default(),
            saves: SaveQueue::default(),
            publish,
        }
    }

    pub fn publish(&self, theme: ThemeFile) {
        (self.publish)(theme);
    }
}

impl<P: Fn(ThemeFile)> Machine for ConfigDriver<P> {
    type Message = ConfigMessage;
    type Effect = Cmd<ConfigEffect, ConfigEvent>;

    fn transition(
        &mut self,
        message: ConfigMessage,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        match message {
            ConfigMessage::Cmds(Cmds { cmds, .. }) => self.commanded(cmds),
            ConfigMessage::Started => {
                let watch = ConfigEffect::Watch(self.directory.clone());
                Ok(Cmd::effect(watch).then(self.poll_everything()))
            }
            ConfigMessage::FilesChanged => Ok(self.poll_everything()),
            ConfigMessage::ReadDone { file, text } => {
                self.drive(WatchMessage::Observed { file, text })
            }
            ConfigMessage::Listed(names) => self.drive(WatchMessage::Listed(names)),
            ConfigMessage::Elapsed(revision) => self.saves.elapsed(revision),
            ConfigMessage::Saved { file, text } => {
                self.drive(WatchMessage::Wrote { file, text })
            }
            ConfigMessage::Changed(change) => Ok(self.changed(change)),
            ConfigMessage::Error(error) => Ok(Cmd::message(ConfigEvent::Error(error))),
        }
    }
}

impl<P: Fn(ThemeFile)> ConfigDriver<P> {
    fn commanded(
        &mut self,
        cmds: Vec<ConfigCmd>,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
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

    fn command(
        &mut self,
        cmd: ConfigCmd,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        match cmd {
            ConfigCmd::Save(patch) => {
                self.saves.queue_config(patch);
                Ok(Cmd::none())
            }
            ConfigCmd::SelectTheme(choice) => {
                self.drive(WatchMessage::SelectTheme(choice.to_string()))
            }
            ConfigCmd::Setting { field, option } => self.setting(field, option),
            ConfigCmd::Flush => Ok(self.saves.flush()),
        }
    }

    fn setting(
        &mut self,
        field: AppearanceField,
        option: OptionIndex,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        if self.appearance == Sighting::First {
            return Err(Unhandled);
        }
        let patch = appearance_patch(field, option).ok_or(Unhandled)?;
        self.appearance_file = self.appearance_file.patched(patch);
        self.saves.queue_appearance(patch);
        let appearance = self.appearance_file.appearance();
        Ok(Cmd::message(ConfigEvent::AppearanceReloaded(appearance)))
    }

    fn drive(
        &mut self,
        message: WatchMessage,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        let (effects, messages) = self.watch.transition(message)?.into_parts();
        let lifted: Cmd<ConfigEffect, ConfigEvent> =
            effects.into_iter().map(lift).collect();
        Ok(messages
            .into_iter()
            .filter_map(|each| self.transition(each).ok())
            .fold(lifted, Cmd::then))
    }

    fn poll_everything(&mut self) -> Cmd<ConfigEffect, ConfigEvent> {
        [
            WatchMessage::Poll(ConfigName::Appearance),
            WatchMessage::Poll(ConfigName::Config),
            WatchMessage::PollTheme,
            WatchMessage::PollThemes,
        ]
        .into_iter()
        .filter_map(|message| self.drive(message).ok())
        .fold(Cmd::none(), Cmd::then)
    }

    fn changed(&mut self, change: ConfigChange) -> Cmd<ConfigEffect, ConfigEvent> {
        match change {
            ConfigChange::Appearance(text) => self.appearance_changed(text.as_deref()),
            ConfigChange::Keymap(text) => self.keymap_changed(text.as_deref()),
            ConfigChange::Theme { name, text } => theme_changed(name, text.as_deref()),
            ConfigChange::Themes(names) => {
                Cmd::message(ConfigEvent::ThemesLoaded(embedded_and_user(names)))
            }
        }
    }

    fn appearance_changed(
        &mut self,
        text: Option<&str>,
    ) -> Cmd<ConfigEffect, ConfigEvent> {
        let parsed =
            text.map_or_else(|| Ok(AppearanceFile::default()), crate::parse_appearance);
        match parsed {
            Ok(file) => {
                let rows = appearance_settings(file.settings());
                let appearance = file.appearance();
                self.appearance_file = file;
                self.appearance = Sighting::Repeat;
                reports([
                    ConfigEvent::AppearanceReloaded(appearance),
                    ConfigEvent::AppearanceSettingsReloaded(rows),
                    reloaded(ConfigName::Appearance, Ok(())),
                ])
            }
            Err(error) => {
                Cmd::message(reloaded(ConfigName::Appearance, Err(invalid(&error))))
            }
        }
    }

    fn keymap_changed(&mut self, text: Option<&str>) -> Cmd<ConfigEffect, ConfigEvent> {
        match crate::parse_config_reload(text.unwrap_or("")) {
            Ok(parsed) => {
                let sighting = std::mem::replace(&mut self.config, Sighting::Repeat);
                if sighting == Sighting::First {
                    return Cmd::none();
                }
                let keymap = ConfigEvent::KeymapReloaded(Box::new(parsed.keymap));
                let music_dir = parsed.music_dir.map(ConfigEvent::MusicDirReloaded);
                reports(std::iter::once(keymap).chain(music_dir))
            }
            Err(error) => {
                Cmd::message(reloaded(ConfigName::Config, Err(invalid(&error))))
            }
        }
    }
}

fn lift(effect: WatchEffect) -> ConfigEffect {
    match effect {
        WatchEffect::Read { file, path } => ConfigEffect::Read { file, path },
        WatchEffect::List(directory) => ConfigEffect::List(directory),
    }
}

fn theme_changed(
    name: ThemeName,
    text: Option<&str>,
) -> Cmd<ConfigEffect, ConfigEvent> {
    match theme_parsed(&name, text) {
        Ok(file) => Cmd::effect(ConfigEffect::Publish(file)).then(reports([
            ConfigEvent::ThemeReloaded(name.clone()),
            reloaded(ConfigName::Theme(name), Ok(())),
        ])),
        Err(error) => Cmd::message(reloaded(ConfigName::Theme(name), Err(error))),
    }
}

fn theme_parsed(
    name: &ThemeName,
    text: Option<&str>,
) -> Result<ThemeFile, ConfigError> {
    let source = text
        .or_else(|| crate::embedded_theme(name.as_str()))
        .ok_or_else(|| ConfigError::Invalid {
            detail: format!("no theme named `{}`", name.as_str()),
        })?;
    crate::parse_theme(source, name.as_str()).map_err(|error| invalid(&error))
}

fn invalid(error: &crate::Error) -> ConfigError {
    ConfigError::Invalid {
        detail: error.to_string(),
    }
}

fn reloaded(name: ConfigName, result: Result<(), ConfigError>) -> ConfigEvent {
    ConfigEvent::Reloaded(ConfigReload { name, result })
}

fn reports(
    events: impl IntoIterator<Item = ConfigEvent>,
) -> Cmd<ConfigEffect, ConfigEvent> {
    events
        .into_iter()
        .map(Cmd::message)
        .fold(Cmd::none(), Cmd::then)
}

fn embedded_and_user(user: Vec<String>) -> Vec<ThemeName> {
    crate::EMBEDDED_THEMES
        .iter()
        .map(|&(name, _)| name.to_string())
        .chain(user)
        .filter_map(|name| ThemeName::new(name).ok())
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

fn config_directory(appearance: &Path) -> PathBuf {
    appearance
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use kernel::{
        Bounded,
        Cmd,
        Cmds,
        ConfigCmd,
        ConfigEvent,
        ConfigPatch,
        ConfigReload,
        IoError,
        Percent,
        domain::{
            ConfigError,
            ConfigName,
            OptionCount,
            OptionIndex,
            Revision,
            ThemeName,
            appearance_rows::{AppearanceField, appearance_patch},
        },
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        ThemeFile,
        driver::{
            ConfigDriver,
            ConfigEffect,
            ConfigMessage,
            ConfigPaths,
            SeenTexts,
            Sighting,
            saves::SAVE_DEBOUNCE,
        },
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    type Driver = ConfigDriver<fn(ThemeFile)>;

    fn driver(theme: Option<&str>) -> Driver {
        let paths = ConfigPaths {
            config: PathBuf::from("/config/config.toml"),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            theme: theme.map(str::to_string),
            seen: SeenTexts::default(),
        };
        ConfigDriver::new(&paths, drop::<ThemeFile>)
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

    fn read_done(file: ConfigName, text: Option<&str>) -> ConfigMessage {
        ConfigMessage::ReadDone {
            file,
            text: text.map(str::to_string),
        }
    }

    fn after(revision: u64) -> Cmd<ConfigEffect, ConfigEvent> {
        Cmd::effect(ConfigEffect::After {
            delay: SAVE_DEBOUNCE,
            revision: (0..revision).fold(Revision::default(), |at, _| at.next()),
        })
    }

    fn reading(file: ConfigName, path: &str) -> ConfigEffect {
        ConfigEffect::Read {
            file,
            path: PathBuf::from(path),
        }
    }

    fn cover_brackets() -> (AppearanceField, OptionIndex) {
        let option = OptionCount::new(2).unwrap().index(1).unwrap();
        (AppearanceField::CoverBrackets, option)
    }

    fn setting() -> ConfigMessage {
        let (field, option) = cover_brackets();
        commanded(vec![ConfigCmd::Setting { field, option }])
    }

    fn appearance_read(mut driver: Driver) -> Driver {
        let read = driver.transition(read_done(ConfigName::Appearance, None));
        assert!(read.is_ok());
        driver
    }

    fn step(
        driver: &mut Driver,
        message: ConfigMessage,
    ) -> Cmd<ConfigEffect, ConfigEvent> {
        driver.transition(message).unwrap()
    }

    #[rstest]
    #[case::started(ConfigMessage::Started, Ok(Cmd::from_iter([
        ConfigEffect::Watch(PathBuf::from("/config")),
        reading(ConfigName::Appearance, "/config/sifr-ui.toml"),
        reading(ConfigName::Config, "/config/config.toml"),
        ConfigEffect::List(PathBuf::from("/config/themes")),
    ])))]
    #[case::first_keymap(
        read_done(ConfigName::Config, Some("[keymap]\nnext = \"x\"\n")),
        Ok(Cmd::none())
    )]
    #[case::own_appearance_write(ConfigMessage::Saved { file: ConfigName::Appearance, text: "[window]\n".to_string() }, Ok(Cmd::none()))]
    #[case::save_error(
        ConfigMessage::Error(ConfigError::Save { file: ConfigName::Config, kind: IoError::Other }),
        Ok(Cmd::message(ConfigEvent::Error(ConfigError::Save { file: ConfigName::Config, kind: IoError::Other }))),
    )]
    #[case::select_theme(commanded(vec![ConfigCmd::SelectTheme("noir".parse().unwrap())]), Ok(Cmd::effect(reading(noir(), "/config/themes/noir.toml"))))]
    #[case::save(saving(ConfigPatch::builder().build()), Ok(after(1)))]
    #[case::setting_before_read(setting(), Err(Unhandled))]
    #[case::nothing_pending(
        ConfigMessage::Elapsed(Revision::default()),
        Err(Unhandled)
    )]
    fn a_fresh_driver_answers(
        #[case] message: ConfigMessage,
        #[case] expected: Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled>,
    ) {
        let mut fresh = driver(None);

        assert_eq!(fresh.transition(message), expected);
    }

    #[test]
    fn the_first_keymap_sighting_is_remembered() {
        let mut next = driver(None);

        let answer = step(&mut next, read_done(ConfigName::Config, None));

        assert_eq!(answer, Cmd::none());
        assert_eq!(next.config, Sighting::Repeat);
    }

    #[test]
    fn a_second_keymap_sighting_tells_keymap_reloaded() {
        let mut settled = driver(None);
        let first = step(
            &mut settled,
            read_done(ConfigName::Config, Some("[keymap]\nnext = \"x\"\n")),
        );
        assert_eq!(first, Cmd::none());

        let (effects, events) = step(
            &mut settled,
            read_done(ConfigName::Config, Some("[keymap]\nnext = \"y\"\n")),
        )
        .into_parts();

        assert!(effects.is_empty());
        assert!(matches!(
            events.as_slice(),
            [ConfigEvent::KeymapReloaded(_)]
        ));
    }

    #[test]
    fn a_changed_theme_publishes_then_tells_theme_reloaded() {
        let mut state = driver(Some("noir"));

        let (effects, events) =
            step(&mut state, read_done(noir(), Some(NOIR_THEME))).into_parts();

        assert!(matches!(effects.as_slice(), [ConfigEffect::Publish(_)]));
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
                result: Err(ConfigError::Invalid { .. }),
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
        assert_eq!(first, after(1));
        for volume in [10_u8, 20, 30, 40, 50] {
            let patch = ConfigPatch::builder()
                .volume(Percent::clamped(volume))
                .build();
            let queued = step(&mut current, saving(patch));
            assert_eq!(
                queued,
                after(u64::from(volume / 10) + 1),
                "every save pushes the trailing edge out"
            );
        }

        let stale =
            current.transition(ConfigMessage::Elapsed(Revision::default().next()));
        let elapsed = ConfigMessage::Elapsed(current.saves.revision());
        let due = step(&mut current, elapsed);

        assert_eq!(stale, Err(Unhandled));
        let expected = ConfigPatch::builder()
            .theme(ThemeName::from_static("noir"))
            .volume(Percent::clamped(50))
            .build();
        assert_eq!(due, Cmd::effect(ConfigEffect::SaveConfig(expected)));
    }

    #[test]
    fn a_setting_tells_the_patched_appearance_and_queues_a_save() {
        let mut next = appearance_read(driver(None));
        let (field, option) = cover_brackets();
        let expected = next
            .appearance_file
            .patched(appearance_patch(field, option).unwrap());

        let (effects, events) = step(&mut next, setting()).into_parts();

        assert!(matches!(
            events.as_slice(),
            [ConfigEvent::AppearanceReloaded(_)]
        ));
        assert_eq!(Cmd::from_iter(effects), after(1));
        assert_eq!(next.appearance_file, expected);
    }

    #[test]
    fn one_batch_saves_both_files_together() {
        let mut next = appearance_read(driver(None));
        let (field, option) = cover_brackets();
        let both = commanded(vec![
            ConfigCmd::Save(ConfigPatch::builder().build()),
            ConfigCmd::Setting { field, option },
        ]);

        let (effects, _) = step(&mut next, both).into_parts();
        let elapsed = ConfigMessage::Elapsed(next.saves.revision());
        let (due, _) = step(&mut next, elapsed).into_parts();

        assert_eq!(Cmd::from_iter(effects), after(2));
        assert!(matches!(
            due.as_slice(),
            [ConfigEffect::SaveConfig(_), ConfigEffect::SaveAppearance(_)]
        ));
    }

    #[rstest]
    #[case::config(vec![ConfigCmd::Save(ConfigPatch::builder().build())], &["save_config"])]
    #[case::appearance(vec![ConfigCmd::Setting { field: cover_brackets().0, option: cover_brackets().1 }], &["save_appearance"])]
    #[case::both(vec![ConfigCmd::Setting { field: cover_brackets().0, option: cover_brackets().1 }, ConfigCmd::Save(ConfigPatch::builder().build())], &["save_config", "save_appearance"])]
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
                if matches!(each, ConfigEffect::SaveConfig(_)) {
                    "save_config"
                } else if matches!(each, ConfigEffect::SaveAppearance(_)) {
                    "save_appearance"
                } else {
                    "other"
                }
            })
            .collect();

        assert_eq!(names, expected);
        assert_eq!(next.transition(ConfigMessage::Elapsed(old)), Err(Unhandled));
    }
}
