use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use config::{AppearanceFile, ThemeFile};
use kernel::{
    ConfigCmd,
    ConfigEvent,
    domain::{
        ConfigError,
        ConfigFile,
        OptionIndex,
        ThemeName,
        appearance_rows::AppearanceField,
    },
    update::Machine,
};
use strum::IntoStaticStr;

use crate::config::{
    ConfigPaths,
    reload::{appearance_reload, config_reload, theme_reload},
    save_queue::{SavePatches, SaveQueue, Saves},
    watch::{ConfigChange, ConfigWatch, ConfigWatchError, WatchEffect, WatchMessage},
    write::{SaveResult, Written},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Sighting {
    #[default]
    First,
    Repeat,
}

pub(crate) const SAVE_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, PartialEq)]
pub(crate) struct ConfigState {
    watch: ConfigWatch,
    config: Sighting,
    appearance_file: AppearanceFile,
    appearance: Sighting,
    saves: SaveQueue,
}

#[derive(Debug, IntoStaticStr)]
pub(crate) enum ConfigMessage {
    Command {
        cmd: ConfigCmd,
        now: Instant,
    },
    FilesChanged,
    Read {
        file: ConfigFile,
        text: Option<String>,
    },
    Unreadable {
        file: ConfigFile,
        detail: String,
    },
    Listed(Result<Vec<String>, String>),
    SaveDue {
        now: Instant,
    },
    Saved(Saves<SaveResult, SaveResult>),
    Stopping,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Published {
    Theme(ThemeFile),
    Appearance(AppearanceFile),
}

#[derive(Debug, PartialEq)]
pub(crate) enum ConfigEffect {
    Read { file: ConfigFile, path: PathBuf },
    List(PathBuf),
    Save(SavePatches),
    Changed(Published),
    Event(ConfigEvent),
}

#[derive(Clone, Copy)]
struct SettingChange {
    field: AppearanceField,
    option: OptionIndex,
}

impl ConfigState {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths, debounce: Duration) -> Self {
        Self {
            watch: ConfigWatch::new(paths),
            config: Sighting::First,
            appearance_file: AppearanceFile::default(),
            appearance: Sighting::First,
            saves: SaveQueue::new(debounce),
        }
    }

    #[must_use]
    pub(crate) fn save_deadline(&self) -> Option<Instant> {
        self.saves.next_deadline()
    }
}

impl Machine for ConfigState {
    type Message = ConfigMessage;
    type Error = ConfigWatchError;
    type Effect = Vec<ConfigEffect>;

    fn transition(
        &mut self,
        input: ConfigMessage,
    ) -> Result<Vec<ConfigEffect>, ConfigWatchError> {
        match input {
            ConfigMessage::Command {
                cmd: ConfigCmd::Save(patch),
                now,
            } => {
                self.saves.queue_config(now, patch);
                Ok(Vec::new())
            }
            ConfigMessage::Command {
                cmd: ConfigCmd::SelectTheme(choice),
                ..
            } => self.drive(WatchMessage::SelectTheme(choice.to_string())),
            ConfigMessage::Command {
                cmd: ConfigCmd::Setting { field, option },
                now,
            } => Ok(self.setting(SettingChange { field, option }, now)),
            ConfigMessage::FilesChanged => Ok(self.poll_everything()),
            ConfigMessage::Read { file, text } => {
                self.drive(WatchMessage::Observed { file, text })
            }
            ConfigMessage::Unreadable { file, detail } => {
                self.drive(WatchMessage::Unreadable { file, detail })
            }
            ConfigMessage::Listed(Ok(names)) => self.drive(WatchMessage::Listed(names)),
            ConfigMessage::Listed(Err(detail)) => {
                self.drive(WatchMessage::ThemesUnreadable(detail))
            }
            ConfigMessage::SaveDue { now } => {
                let writes = self.saves.take_due(now);
                Ok(writes.map(ConfigEffect::Save).into_iter().collect())
            }
            ConfigMessage::Saved(results) => Ok(self.saved(results)),
            ConfigMessage::Stopping => {
                let writes = self.saves.take_all();
                Ok(writes.map(ConfigEffect::Save).into_iter().collect())
            }
        }
    }
}

impl ConfigState {
    fn drive(
        &mut self,
        message: WatchMessage,
    ) -> Result<Vec<ConfigEffect>, ConfigWatchError> {
        let io = self.watch.transition(message)?;
        Ok(self.react(io))
    }

    fn react(&mut self, io: WatchEffect) -> Vec<ConfigEffect> {
        match io {
            WatchEffect::Nothing => Vec::new(),
            WatchEffect::Read { file, path } => vec![ConfigEffect::Read { file, path }],
            WatchEffect::List(dir) => vec![ConfigEffect::List(dir)],
            WatchEffect::Changed(change) => self.changed(change),
        }
    }

    fn changed(&mut self, change: ConfigChange) -> Vec<ConfigEffect> {
        match change {
            ConfigChange::Appearance(text) => self.appearance_changed(text.as_deref()),
            ConfigChange::Keymap(text) => self.keymap_changed(text.as_deref()),
            ConfigChange::Theme { name, text } => theme_changed(&name, text.as_deref()),
            ConfigChange::Themes(names) => {
                vec![ConfigEffect::Event(ConfigEvent::ThemesLoaded(
                    embedded_and_user(names),
                ))]
            }
            ConfigChange::ThemesUnreadable(detail) => {
                vec![ConfigEffect::Event(ConfigEvent::Error(
                    ConfigError::ThemesUnreadable { detail },
                ))]
            }
            ConfigChange::Unreadable { file, detail } => {
                vec![ConfigEffect::Event(ConfigEvent::Error(
                    ConfigError::Unreadable { file, detail },
                ))]
            }
        }
    }

    fn setting(&mut self, change: SettingChange, now: Instant) -> Vec<ConfigEffect> {
        let SettingChange { field, option } = change;
        if self.appearance == Sighting::First {
            return Vec::new();
        }
        match kernel::domain::appearance_rows::appearance_patch(field, option) {
            Some(patch) => {
                self.appearance_file = self.appearance_file.patched(patch);
                self.saves.queue_appearance(now, patch);
                let published = self.appearance_file.clone();
                vec![ConfigEffect::Changed(Published::Appearance(published))]
            }
            None => Vec::new(),
        }
    }

    fn keymap_changed(&mut self, text: Option<&str>) -> Vec<ConfigEffect> {
        match config_reload(text) {
            Ok(parsed) => {
                let sighting = std::mem::replace(&mut self.config, Sighting::Repeat);
                if sighting == Sighting::First {
                    return Vec::new();
                }
                std::iter::once(ConfigEffect::Event(ConfigEvent::KeymapReloaded(
                    Box::new(parsed.keymap),
                )))
                .chain(
                    parsed.music_dir.map(|dir| {
                        ConfigEffect::Event(ConfigEvent::MusicDirReloaded(dir))
                    }),
                )
                .collect()
            }
            Err(error) => vec![source_failed(ConfigFile::Config, error.to_string())],
        }
    }

    fn poll_everything(&mut self) -> Vec<ConfigEffect> {
        [
            WatchMessage::Poll(ConfigFile::Appearance),
            WatchMessage::Poll(ConfigFile::Config),
            WatchMessage::Poll(ConfigFile::Theme),
            WatchMessage::PollThemes,
        ]
        .into_iter()
        .filter_map(|message| self.drive(message).ok())
        .flatten()
        .collect()
    }

    fn saved(&mut self, results: Saves<SaveResult, SaveResult>) -> Vec<ConfigEffect> {
        let files = match results {
            Saves::Config(config) => vec![(ConfigFile::Config, config)],
            Saves::Appearance(appearance) => {
                vec![(ConfigFile::Appearance, appearance)]
            }
            Saves::Both { config, appearance } => vec![
                (ConfigFile::Config, config),
                (ConfigFile::Appearance, appearance),
            ],
        };
        files
            .into_iter()
            .flat_map(|(file, result)| self.apply_save_result(file, result))
            .collect()
    }

    fn apply_save_result(
        &mut self,
        file: ConfigFile,
        result: SaveResult,
    ) -> Vec<ConfigEffect> {
        match result {
            Ok(Written { text }) => self
                .drive(WatchMessage::Wrote { file, text })
                .unwrap_or_else(|_| Vec::new()),
            Err(error) => {
                vec![ConfigEffect::Event(ConfigEvent::Error(ConfigError::Save {
                    file,
                    detail: error.to_string(),
                }))]
            }
        }
    }
}

impl ConfigState {
    fn appearance_changed(&mut self, text: Option<&str>) -> Vec<ConfigEffect> {
        match appearance_reload(text) {
            Ok(file) => {
                let rows = kernel::domain::appearance_rows::appearance_settings(
                    file.appearance(),
                );
                self.appearance_file = file.clone();
                self.appearance = Sighting::Repeat;
                vec![
                    ConfigEffect::Changed(Published::Appearance(file)),
                    ConfigEffect::Event(ConfigEvent::AppearanceSettingsReloaded(rows)),
                    source_recovered(ConfigFile::Appearance),
                ]
            }
            Err(error) => {
                vec![source_failed(ConfigFile::Appearance, error.to_string())]
            }
        }
    }
}

fn theme_changed(name: &str, text: Option<&str>) -> Vec<ConfigEffect> {
    match theme_reload(name, text) {
        Ok(file) => std::iter::once(ConfigEffect::Changed(Published::Theme(file)))
            .chain(ThemeName::new(name.to_string()).ok().map(|theme_name| {
                ConfigEffect::Event(ConfigEvent::ThemeReloaded(theme_name))
            }))
            .chain(std::iter::once(source_recovered(ConfigFile::Theme)))
            .collect(),
        Err(error) => vec![source_failed(ConfigFile::Theme, error.to_string())],
    }
}

fn source_recovered(source: ConfigFile) -> ConfigEffect {
    ConfigEffect::Event(ConfigEvent::SourceRecovered(source))
}

fn source_failed(source: ConfigFile, text: String) -> ConfigEffect {
    ConfigEffect::Event(ConfigEvent::SourceFailed { source, text })
}

fn embedded_and_user(user: Vec<String>) -> Vec<ThemeName> {
    config::EMBEDDED_THEMES
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

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use kernel::{
        Bounded,
        ConfigCmd,
        ConfigPatch,
        Percent,
        domain::{ConfigError, ConfigFile, OptionCount, ThemeName},
        update::Machine,
    };

    use crate::{
        config::{
            ConfigPaths,
            machine::{
                ConfigEffect,
                ConfigMessage,
                ConfigState,
                Published,
                SAVE_DEBOUNCE,
                Sighting,
            },
            save_queue::{SavePatches, Saves},
            write::Written,
        },
        error::SaveError,
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    fn paths(theme: Option<&str>) -> ConfigPaths {
        ConfigPaths {
            config: PathBuf::from("/config/config.toml"),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            theme: theme.map(str::to_string),
            seen: crate::config::SeenTexts::default(),
        }
    }

    fn driver(theme: Option<&str>) -> ConfigState {
        ConfigState::new(&paths(theme), SAVE_DEBOUNCE)
    }

    fn save(mut state: ConfigState, patch: ConfigPatch, at: Instant) -> ConfigState {
        let outputs = state
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Save(patch),
                now: at,
            })
            .unwrap();
        assert!(outputs.is_empty());
        state
    }

    fn with_appearance_read(mut state: ConfigState) -> ConfigState {
        state
            .transition(ConfigMessage::Read {
                file: ConfigFile::Appearance,
                text: None,
            })
            .unwrap();
        state
    }

    #[test]
    fn the_first_keymap_sighting_tells_nothing() {
        let mut next = driver(None);
        let outputs = next
            .transition(ConfigMessage::Read {
                file: ConfigFile::Config,
                text: Some("[keymap]\nnext = \"x\"\n".to_string()),
            })
            .unwrap();

        assert!(outputs.is_empty());
        assert_eq!(next.config, Sighting::Repeat);
    }

    #[test]
    fn a_second_keymap_sighting_tells_keymap_reloaded() {
        let mut settled = driver(None);
        let _ = settled
            .transition(ConfigMessage::Read {
                file: ConfigFile::Config,
                text: Some("[keymap]\nnext = \"x\"\n".to_string()),
            })
            .unwrap();

        let mut state = settled;
        let outputs = state
            .transition(ConfigMessage::Read {
                file: ConfigFile::Config,
                text: Some("[keymap]\nnext = \"y\"\n".to_string()),
            })
            .unwrap();

        assert!(outputs.iter().any(|output| matches!(
            output,
            ConfigEffect::Event(kernel::ConfigEvent::KeymapReloaded(_))
        )));
    }

    #[test]
    fn a_changed_theme_publishes_then_tells_theme_reloaded() {
        let mut state = driver(Some("noir"));
        let outputs = state
            .transition(ConfigMessage::Read {
                file: ConfigFile::Theme,
                text: Some(NOIR_THEME.to_string()),
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                ConfigEffect::Changed(Published::Theme(_)),
                ConfigEffect::Event(kernel::ConfigEvent::ThemeReloaded(_)),
                ConfigEffect::Event(kernel::ConfigEvent::SourceRecovered(
                    ConfigFile::Theme
                )),
            ]
        ));
    }

    #[test]
    fn an_unparsable_appearance_tells_config_failed() {
        let mut state = driver(None);
        let outputs = state
            .transition(ConfigMessage::Read {
                file: ConfigFile::Appearance,
                text: Some("[cover\nnot toml".to_string()),
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigEffect::Event(kernel::ConfigEvent::SourceFailed {
                source: ConfigFile::Appearance,
                ..
            })]
        ));
    }

    #[test]
    fn a_failed_save_tells_save_failed() {
        let saved = ConfigMessage::Saved(Saves::Config(Err(SaveError::Read {
            path: PathBuf::from("/config/config.toml"),
            source: std::io::Error::other("denied"),
        })));

        let mut state = driver(None);
        let outputs = state.transition(saved).unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigEffect::Event(kernel::ConfigEvent::Error(
                ConfigError::Save {
                    file: ConfigFile::Config,
                    ..
                }
            ))]
        ));
    }

    #[test]
    fn a_successful_appearance_save_tells_nothing() {
        let saved = ConfigMessage::Saved(Saves::Appearance(Ok(Written {
            text: "[window]\nkey_hints = true\n".to_string(),
        })));

        let mut state = driver(None);
        let outputs = state.transition(saved).unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn select_theme_reads_the_theme_file() {
        let mut state = driver(None);
        let outputs = state
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::SelectTheme("noir".parse().unwrap()),
                now: Instant::now(),
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigEffect::Read {
                file: ConfigFile::Theme,
                ..
            }]
        ));
    }

    #[test]
    fn stopping_with_nothing_pending_writes_nothing() {
        let mut state = driver(None);
        let outputs = state.transition(ConfigMessage::Stopping).unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn stopping_writes_a_save_whose_window_has_not_elapsed() {
        let patch = ConfigPatch::builder()
            .theme(ThemeName::from_static("dark"))
            .build();
        let pending = save(driver(None), patch.clone(), Instant::now());

        let mut next = pending;
        let outputs = next.transition(ConfigMessage::Stopping).unwrap();

        assert_eq!(
            outputs,
            vec![ConfigEffect::Save(SavePatches::Config(patch))]
        );
        assert_eq!(next.save_deadline(), None);
    }

    #[test]
    fn a_burst_of_saves_becomes_one_write_carrying_every_field() {
        let start = Instant::now();
        let mut current = save(
            driver(None),
            ConfigPatch::builder()
                .theme(ThemeName::from_static("noir"))
                .build(),
            start,
        );
        for volume in [10u8, 20, 30, 40, 50] {
            let at = start + Duration::from_millis(u64::from(volume));
            let patch = ConfigPatch::builder()
                .volume(Percent::clamped(volume))
                .build();
            current = save(current, patch, at);
            assert_eq!(
                current.save_deadline(),
                Some(at + SAVE_DEBOUNCE),
                "every new save in the burst pushes the trailing edge out"
            );
        }
        let last = start + Duration::from_millis(50);

        let early = current
            .transition(ConfigMessage::SaveDue { now: last })
            .unwrap();
        assert!(early.is_empty(), "the window has not elapsed yet");
        let due = current
            .transition(ConfigMessage::SaveDue {
                now: last + SAVE_DEBOUNCE,
            })
            .unwrap();

        let expected = ConfigPatch::builder()
            .theme(ThemeName::from_static("noir"))
            .volume(Percent::clamped(50))
            .build();
        let patches = SavePatches::Config(expected);
        assert_eq!(due, vec![ConfigEffect::Save(patches)]);
        assert_eq!(current.save_deadline(), None);
    }

    #[test]
    fn a_setting_publishes_the_patched_appearance_and_queues_a_save() {
        let seeded = with_appearance_read(driver(None));
        let option = OptionCount::new(2).unwrap().index(1).unwrap();

        let patch = kernel::domain::appearance_rows::appearance_patch(
            kernel::domain::appearance_rows::AppearanceField::CoverBrackets,
            option,
        )
        .unwrap();
        let expected = seeded.appearance_file.patched(patch);
        let at = Instant::now();

        let mut next = seeded;
        let outputs = next
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Setting {
                    field:
                        kernel::domain::appearance_rows::AppearanceField::CoverBrackets,
                    option,
                },
                now: at,
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigEffect::Changed(Published::Appearance(_))]
        ));
        assert_eq!(next.save_deadline(), Some(at + SAVE_DEBOUNCE));
        assert_eq!(next.appearance_file, expected);
    }

    #[test]
    fn a_setting_before_the_first_appearance_read_is_dropped() {
        let mut next = driver(None);
        let outputs = next
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Setting {
                    field:
                        kernel::domain::appearance_rows::AppearanceField::CoverBrackets,
                    option: OptionCount::new(2).unwrap().index(1).unwrap(),
                },
                now: Instant::now(),
            })
            .unwrap();

        assert!(outputs.is_empty());
        assert_eq!(next.save_deadline(), None);
    }

    #[test]
    fn a_command_save_queues_the_patch() {
        let at = Instant::now();

        let next = save(driver(None), ConfigPatch::builder().build(), at);

        assert_eq!(next.save_deadline(), Some(at + SAVE_DEBOUNCE));
    }
}
