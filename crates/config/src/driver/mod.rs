pub mod effect;
mod execute;
mod files;
pub mod message;
pub mod paths;
mod saves;
mod watch;

use std::path::{Path, PathBuf};

use kernel::{
    cmd::{Cmd, Cmds, ConfigCmd},
    domain::{
        appearance_rows::appearance_patch,
        config::{ConfigError, ConfigName, Diagnostic},
        io_error::IoError,
        setting_row::{AppearanceField, OptionIndex},
        theme::{ThemeChoice, ThemeName},
    },
    message::{ConfigEvent, ConfigReload},
    update::machine::{Machine, Unhandled},
};

use crate::{
    appearance_file::TomlAppearance,
    driver::{
        effect::ConfigEffect,
        message::{ConfigChange, ConfigMessage},
        paths::ConfigPaths,
        saves::PendingSaves,
        watch::{ConfigWatch, ConfigWatchMessage, WatchEffect},
    },
    theme_file::TomlTheme,
};

pub struct ConfigDriver<P: Fn(TomlTheme), A: Fn(TomlAppearance)> {
    directory: Option<PathBuf>,
    config: PathBuf,
    appearance: PathBuf,
    watch: ConfigWatch,
    appearance_file: TomlAppearance,
    saves: PendingSaves,
    publish_theme: P,
    publish_appearance: A,
}

impl<P: Fn(TomlTheme), A: Fn(TomlAppearance)> std::fmt::Debug for ConfigDriver<P, A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigDriver")
            .field("directory", &self.directory)
            .field("watch", &self.watch)
            .field("saves", &self.saves)
            .finish_non_exhaustive()
    }
}

impl<P: Fn(TomlTheme), A: Fn(TomlAppearance)> ConfigDriver<P, A> {
    pub fn new(paths: &ConfigPaths, publish_theme: P, publish_appearance: A) -> Self {
        Self {
            directory: config_directory(&paths.appearance),
            config: paths.config.clone(),
            appearance: paths.appearance.clone(),
            watch: ConfigWatch::new(paths),
            appearance_file: seeded_appearance(paths.seen.appearance.as_deref()),
            saves: PendingSaves::default(),
            publish_theme,
            publish_appearance,
        }
    }
}

impl<P: Fn(TomlTheme), A: Fn(TomlAppearance)> Machine for ConfigDriver<P, A> {
    type Message = ConfigMessage;
    type Effect = Cmd<ConfigEffect, ConfigEvent>;

    fn transition(
        &mut self,
        message: ConfigMessage,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        match message {
            ConfigMessage::Cmds(Cmds { cmds, .. }) => self.commanded(cmds),
            ConfigMessage::Started => {
                let watch = self.directory.clone().map_or_else(
                    || {
                        Cmd::message(ConfigEvent::Error(ConfigError::Watch(
                            IoError::Missing,
                        )))
                    },
                    |directory| Cmd::effect(ConfigEffect::Watch(directory)),
                );
                Ok(watch.then(self.poll_everything()))
            }
            ConfigMessage::Changed(Ok(())) => Ok(self.poll_everything()),
            ConfigMessage::Changed(Err(kind)) => {
                Ok(Cmd::message(ConfigEvent::Error(ConfigError::Watch(kind))))
            }
            ConfigMessage::ReadDone { file, text } => {
                self.drive_watch(ConfigWatchMessage::Observed { file, text })
            }
            ConfigMessage::Listed(names) => {
                self.drive_watch(ConfigWatchMessage::Listed(names))
            }
            ConfigMessage::Elapsed(revision) => self.saves.elapsed(revision),
            ConfigMessage::Saved { file, text } => {
                self.drive_watch(ConfigWatchMessage::Wrote { file, text })
            }
            ConfigMessage::Reloaded(change) => Ok(self.changed(change)),
            ConfigMessage::Error(error) => Ok(Cmd::message(ConfigEvent::Error(error))),
        }
    }
}

impl<P: Fn(TomlTheme), A: Fn(TomlAppearance)> ConfigDriver<P, A> {
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
                self.saves.hold_config(patch);
                Ok(Cmd::none())
            }
            ConfigCmd::SelectTheme(ThemeChoice::Named(name)) => {
                self.drive_watch(ConfigWatchMessage::SelectTheme(name))
            }
            ConfigCmd::SelectTheme(ThemeChoice::Auto) => Err(Unhandled),
            ConfigCmd::SetAppearance { field, option } => self.setting(field, option),
            ConfigCmd::Flush => Ok(self.saves.flush()),
        }
    }

    fn setting(
        &mut self,
        field: AppearanceField,
        option: OptionIndex,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        let patch = appearance_patch(field, option).ok_or(Unhandled)?;
        self.appearance_file = self.appearance_file.patched(patch);
        self.saves.hold_appearance(patch);
        let settings = self.appearance_file.settings();
        Ok(Cmd::message(ConfigEvent::AppearanceReloaded(settings)))
    }

    fn drive_watch(
        &mut self,
        message: ConfigWatchMessage,
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
            ConfigWatchMessage::PollAppearance,
            ConfigWatchMessage::PollConfig,
            ConfigWatchMessage::PollTheme,
            ConfigWatchMessage::PollThemes,
        ]
        .into_iter()
        .filter_map(|message| self.drive_watch(message).ok())
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
        let parsed = text.map_or_else(
            || Ok(TomlAppearance::default()),
            crate::appearance_file::parse_appearance,
        );
        match parsed {
            Ok(file) => {
                let settings = file.settings();
                self.appearance_file = file.clone();
                Cmd::effect(ConfigEffect::PublishAppearance(file)).then(reports([
                    ConfigEvent::AppearanceReloaded(settings),
                    reloaded(ConfigName::Appearance, Ok(())),
                ]))
            }
            Err(error) => {
                Cmd::message(reloaded(ConfigName::Appearance, Err(invalid(&error))))
            }
        }
    }

    fn keymap_changed(&self, text: Option<&str>) -> Cmd<ConfigEffect, ConfigEvent> {
        match crate::config_file::parse_config_reload(text.unwrap_or("")) {
            Ok(parsed) => {
                let keymap = ConfigEvent::KeymapReloaded(Box::new(parsed.keymap));
                let music_dir = parsed
                    .music_dir
                    .or_else(dirs::audio_dir)
                    .map(ConfigEvent::MusicDirReloaded);
                reports(
                    std::iter::once(keymap)
                        .chain(music_dir)
                        .chain([reloaded(ConfigName::Config, Ok(()))]),
                )
            }
            Err(error) => {
                Cmd::message(reloaded(ConfigName::Config, Err(invalid(&error))))
            }
        }
    }
}

fn seeded_appearance(text: Option<&str>) -> TomlAppearance {
    match text.map(crate::appearance_file::parse_appearance) {
        Some(Ok(file)) => file,
        Some(Err(_)) | None => TomlAppearance::default(),
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
        Ok(file) => Cmd::effect(ConfigEffect::PublishTheme(file)).then(reports([
            ConfigEvent::ThemeReloaded(name.clone()),
            reloaded(ConfigName::Theme(name), Ok(())),
        ])),
        Err(error) => Cmd::message(reloaded(ConfigName::Theme(name), Err(error))),
    }
}

fn theme_parsed(
    name: &ThemeName,
    text: Option<&str>,
) -> Result<TomlTheme, ConfigError> {
    let source = text
        .or_else(|| crate::embedded_theme::embedded_theme(name.as_str()))
        .ok_or_else(|| invalid(&crate::error::Error::UnknownTheme(name.clone())))?;
    crate::theme_file::parse_theme(source, name.as_str())
        .map_err(|error| invalid(&error))
}

fn invalid(error: &crate::error::Error) -> ConfigError {
    ConfigError::Invalid(Diagnostic::from_error(error))
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
    use std::{path::PathBuf, time::Instant};

    use kernel::{
        cmd::{Cmd, Cmds, ConfigCmd, ConfigPatch},
        domain::{
            appearance_rows::appearance_patch,
            bounded::Bounded,
            config::{ConfigError, ConfigName},
            io_error::IoError,
            percent::Percent,
            revision::Revision,
            setting_row::{AppearanceField, OptionCount, OptionIndex},
            theme::{ThemeChoice, ThemeName},
        },
        message::{ConfigEvent, ConfigReload},
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        appearance_file::{TomlAppearance, parse_appearance},
        driver::{
            ConfigDriver,
            effect::ConfigEffect,
            message::ConfigMessage,
            paths::{ConfigPaths, SeenTexts},
            saves::SAVE_DEBOUNCE,
        },
        theme_file::TomlTheme,
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbackground = \"#000000\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    const PLAIN_COVER: &str = "[cover]\nmode = \"plain\"\n";
    const KEYS_X: &str = "[keymap]\nnext = \"x\"\n";
    const KEYS_Y: &str = "[keymap]\nnext = \"y\"\n";

    type Driver = ConfigDriver<fn(TomlTheme), fn(TomlAppearance)>;

    fn seeded(theme: Option<&'static str>, seen: SeenTexts) -> Driver {
        let paths = ConfigPaths {
            config: PathBuf::from("/config/config.toml"),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            theme: theme.map(ThemeName::from_static),
            seen,
        };
        ConfigDriver::new(&paths, drop::<TomlTheme>, drop::<TomlAppearance>)
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
        commanded(vec![ConfigCmd::SetAppearance { field, option }])
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
    #[case::select_theme(commanded(vec![ConfigCmd::SelectTheme("noir".parse().unwrap())]), Ok(Cmd::effect(reading(noir(), "/config/themes/noir.toml"))))]
    #[case::save(saving(ConfigPatch::builder().build()), Ok(after(1)))]
    fn a_fresh_driver_answers(
        #[case] message: ConfigMessage,
        #[case] expected: Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled>,
    ) {
        let mut fresh = driver(None);

        assert_eq!(fresh.transition(message), expected);
    }

    #[rstest]
    #[case::nothing_pending(ConfigMessage::Elapsed(Revision::default()))]
    #[case::auto_theme(commanded(vec![ConfigCmd::SelectTheme(ThemeChoice::Auto)]))]
    fn a_fresh_driver_refuses_and_stays_unchanged(#[case] message: ConfigMessage) {
        let mut fresh = driver(None);
        let before = format!("{fresh:?}");

        assert_eq!(fresh.transition(message), Err(Unhandled));
        assert_eq!(format!("{fresh:?}"), before);
    }

    #[test]
    fn the_first_external_config_edit_reloads_and_clears_the_error() {
        let seen = SeenTexts {
            config: Some(KEYS_X.to_string()),
            ..SeenTexts::default()
        };
        let mut settled = seeded(None, seen);
        let startup_read =
            step(&mut settled, read_done(ConfigName::Config, Some(KEYS_X)));
        assert_eq!(startup_read, Cmd::none());

        let (effects, events) =
            step(&mut settled, read_done(ConfigName::Config, Some(KEYS_Y)))
                .into_parts();

        assert!(effects.is_empty());
        let default_dir = dirs::audio_dir().map(ConfigEvent::MusicDirReloaded);
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
            [ConfigEffect::PublishTheme(_)]
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
        assert!(started_effects.contains(&reading(noir(), "/config/themes/noir.toml")));

        let (effects, _) =
            step(&mut state, read_done(noir(), Some(NOIR_THEME))).into_parts();

        assert!(matches!(
            effects.as_slice(),
            [ConfigEffect::PublishTheme(_)]
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
            [ConfigEffect::PublishAppearance(_)]
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

        let before_stale = format!("{current:?}");
        let stale =
            current.transition(ConfigMessage::Elapsed(Revision::default().next()));
        assert_eq!(format!("{current:?}"), before_stale);
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
    fn a_setting_with_the_file_present_at_startup_saves_and_tells_the_patch() {
        let seen = SeenTexts {
            appearance: Some(PLAIN_COVER.to_string()),
            ..SeenTexts::default()
        };
        let mut next = seeded(None, seen);
        let startup_read = step(
            &mut next,
            read_done(ConfigName::Appearance, Some(PLAIN_COVER)),
        );
        assert_eq!(startup_read, Cmd::none());
        let (field, option) = cover_brackets();
        let patched = parse_appearance(PLAIN_COVER)
            .unwrap()
            .patched(appearance_patch(field, option).unwrap());

        let (effects, events) = step(&mut next, setting()).into_parts();

        assert_eq!(
            events,
            [ConfigEvent::AppearanceReloaded(patched.settings())]
        );
        assert_eq!(Cmd::from_iter(effects), after(1));
    }

    #[test]
    fn one_batch_saves_both_files_together() {
        let mut next = appearance_read(driver(None));
        let (field, option) = cover_brackets();
        let both = commanded(vec![
            ConfigCmd::Save(ConfigPatch::builder().build()),
            ConfigCmd::SetAppearance { field, option },
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
    #[case::appearance(vec![ConfigCmd::SetAppearance { field: cover_brackets().0, option: cover_brackets().1 }], &["save_appearance"])]
    #[case::both(vec![ConfigCmd::SetAppearance { field: cover_brackets().0, option: cover_brackets().1 }, ConfigCmd::Save(ConfigPatch::builder().build())], &["save_config", "save_appearance"])]
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
        let flushed_state = format!("{next:?}");
        assert_eq!(next.transition(ConfigMessage::Elapsed(old)), Err(Unhandled));
        assert_eq!(format!("{next:?}"), flushed_state);
    }
}
