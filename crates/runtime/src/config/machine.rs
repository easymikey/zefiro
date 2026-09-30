use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use config::{AppearanceFile, ThemeFile};
use kernel::{
    ConfigCmd,
    ConfigEvent,
    domain::{ConfigError, ConfigFile, OptionIndex, SettingId, ThemeName},
    update::{Machine, Rejected},
};
use strum::IntoStaticStr;

use crate::{
    config::{
        ConfigPaths,
        disk::Listing,
        reload::{appearance_reload, config_reload, theme_reload},
        save_queue::{SavePatches, SaveQueue, Saved},
        watch::{
            ConfigChange,
            ConfigWatch,
            ConfigWatchError,
            WatchEffect,
            WatchMessage,
        },
        write::Written,
    },
    error::SaveError,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Sighting {
    #[default]
    First,
    Repeat,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ConfigState {
    watch: Box<ConfigWatch>,
    config: Sighting,
    appearance_file: Box<AppearanceFile>,
    appearance: Sighting,
    saves: Box<SaveQueue>,
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
    Listed(Listing),
    SaveDue {
        now: Instant,
    },
    Saved(Saved),
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
    id: SettingId,
    option: OptionIndex,
}

#[derive(Clone, Copy)]
struct SaveTarget {
    file: ConfigFile,
    wrote: fn(String) -> WatchMessage,
}

impl ConfigState {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths, debounce: Duration) -> Self {
        Self {
            watch: Box::new(ConfigWatch::new(paths)),
            config: Sighting::First,
            appearance_file: Box::default(),
            appearance: Sighting::First,
            saves: Box::new(SaveQueue::new(debounce)),
        }
    }

    #[must_use]
    pub(crate) fn save_deadline(&self) -> Option<Instant> {
        self.saves.next_deadline()
    }
}

type Step = Result<(ConfigState, Vec<ConfigEffect>), Rejected<ConfigState>>;

impl Machine for ConfigState {
    type Message = ConfigMessage;
    type Error = ConfigWatchError;
    type Effect = Vec<ConfigEffect>;

    fn transition(mut self, input: ConfigMessage) -> Step {
        match input {
            ConfigMessage::Command {
                cmd: ConfigCmd::Save(patch),
                now,
            } => {
                self.saves.queue_config(now, patch);
                Ok((self, Vec::new()))
            }
            ConfigMessage::Command {
                cmd: ConfigCmd::SelectTheme(choice),
                ..
            } => self.drive(WatchMessage::SelectTheme(choice.to_string())),
            ConfigMessage::Command {
                cmd: ConfigCmd::Setting { id, option },
                now,
            } => Ok(self.setting(SettingChange { id, option }, now)),
            ConfigMessage::FilesChanged => Ok(self.poll_everything()),
            ConfigMessage::Read { file, text } => {
                self.drive(WatchMessage::Observed { file, text })
            }
            ConfigMessage::Unreadable { file, detail } => {
                self.drive(WatchMessage::Unreadable { file, detail })
            }
            ConfigMessage::Listed(Listing::Names(names)) => {
                self.drive(WatchMessage::Listed(names))
            }
            ConfigMessage::Listed(Listing::Unreadable(detail)) => {
                self.drive(WatchMessage::ThemesUnreadable(detail))
            }
            ConfigMessage::SaveDue { now } => {
                let writes = self.saves.take_due(now);
                Ok((self, write_outputs(writes)))
            }
            ConfigMessage::Saved(flushed) => Ok(self.saved(flushed)),
            ConfigMessage::Stopping => {
                let writes = self.saves.take_all();
                Ok((self, write_outputs(writes)))
            }
        }
    }
}

impl ConfigState {
    fn drive(mut self, message: WatchMessage) -> Step {
        let watch = std::mem::take(&mut *self.watch);
        match watch.transition(message) {
            Ok((watch, io)) => {
                *self.watch = watch;
                let outputs = self.react(io);
                Ok((self, outputs))
            }
            Err(Rejected { state, reason }) => {
                *self.watch = state;
                Err(Rejected {
                    state: self,
                    reason,
                })
            }
        }
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
                vec![emit(ConfigEvent::ThemesLoaded(embedded_and_user(names)))]
            }
            ConfigChange::ThemesUnreadable(detail) => {
                vec![emit(ConfigEvent::Error(ConfigError::ThemesUnreadable {
                    detail,
                }))]
            }
            ConfigChange::Unreadable { file, detail } => {
                vec![emit(ConfigEvent::Error(ConfigError::Unreadable {
                    file,
                    detail,
                }))]
            }
        }
    }

    fn setting(
        mut self,
        change: SettingChange,
        now: Instant,
    ) -> (Self, Vec<ConfigEffect>) {
        let SettingChange { id, option } = change;
        if self.appearance == Sighting::First {
            return (self, Vec::new());
        }
        match config::appearance_patch(id, option) {
            Ok(patch) => {
                *self.appearance_file = self.appearance_file.patched(patch);
                self.saves.queue_appearance(now, patch);
                let published = (*self.appearance_file).clone();
                let outputs =
                    vec![ConfigEffect::Changed(Published::Appearance(published))];
                (self, outputs)
            }
            Err(_) => (self, Vec::new()),
        }
    }

    fn keymap_changed(&mut self, text: Option<&str>) -> Vec<ConfigEffect> {
        match config_reload(text) {
            Ok(parsed) => {
                let sighting = std::mem::replace(&mut self.config, Sighting::Repeat);
                if sighting == Sighting::First {
                    return Vec::new();
                }
                let mut outputs =
                    vec![emit(ConfigEvent::KeymapReloaded(Box::new(parsed.keymap)))];
                if let Some(music_dir) = parsed.music_dir {
                    outputs.push(emit(ConfigEvent::MusicDirReloaded(music_dir)));
                }
                outputs
            }
            Err(error) => vec![source_failed(ConfigFile::Config, error.to_string())],
        }
    }

    fn poll_everything(mut self) -> (Self, Vec<ConfigEffect>) {
        let mut outputs = Vec::new();
        for message in [
            WatchMessage::Poll(ConfigFile::Appearance),
            WatchMessage::Poll(ConfigFile::Config),
            WatchMessage::Poll(ConfigFile::Theme),
            WatchMessage::PollThemes,
        ] {
            match self.drive(message) {
                Ok((next, mut more)) => {
                    self = next;
                    outputs.append(&mut more);
                }
                Err(rejected) => self = rejected.state,
            }
        }
        (self, outputs)
    }

    fn saved(mut self, flushed: Saved) -> (Self, Vec<ConfigEffect>) {
        let mut outputs = Vec::new();
        if let Some(result) = flushed.config {
            let target = SaveTarget {
                file: ConfigFile::Config,
                wrote: WatchMessage::WroteConfig,
            };
            let (next, mut more) = self.apply_save_result(result, target);
            self = next;
            outputs.append(&mut more);
        }
        if let Some(result) = flushed.appearance {
            let target = SaveTarget {
                file: ConfigFile::Appearance,
                wrote: WatchMessage::WroteAppearance,
            };
            let (next, mut more) = self.apply_save_result(result, target);
            self = next;
            outputs.append(&mut more);
        }
        (self, outputs)
    }

    fn apply_save_result(
        self,
        result: Result<Written, SaveError>,
        target: SaveTarget,
    ) -> (Self, Vec<ConfigEffect>) {
        match result {
            Ok(written) => match self.drive((target.wrote)(written.text)) {
                Ok(pair) => pair,
                Err(rejected) => (rejected.state, Vec::new()),
            },
            Err(error) => (
                self,
                vec![emit(ConfigEvent::Error(ConfigError::Save {
                    file: target.file,
                    detail: error.to_string(),
                }))],
            ),
        }
    }
}

impl ConfigState {
    fn appearance_changed(&mut self, text: Option<&str>) -> Vec<ConfigEffect> {
        match appearance_reload(text) {
            Ok(file) => {
                let rows = config::custom_settings(&file);
                *self.appearance_file = file.clone();
                self.appearance = Sighting::Repeat;
                vec![
                    ConfigEffect::Changed(Published::Appearance(file)),
                    emit(ConfigEvent::CustomRowsReloaded(rows)),
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
        Ok(file) => {
            let mut outputs = vec![ConfigEffect::Changed(Published::Theme(file))];
            if let Ok(theme_name) = ThemeName::new(name.to_string()) {
                outputs.push(emit(ConfigEvent::ThemeReloaded(theme_name)));
            }
            outputs.push(source_recovered(ConfigFile::Theme));
            outputs
        }
        Err(error) => vec![source_failed(ConfigFile::Theme, error.to_string())],
    }
}

fn write_outputs(writes: Option<SavePatches>) -> Vec<ConfigEffect> {
    writes.map(ConfigEffect::Save).into_iter().collect()
}

fn source_recovered(source: ConfigFile) -> ConfigEffect {
    emit(ConfigEvent::SourceRecovered(source))
}

fn source_failed(source: ConfigFile, text: String) -> ConfigEffect {
    emit(ConfigEvent::SourceFailed { source, text })
}

fn emit(event: ConfigEvent) -> ConfigEffect {
    ConfigEffect::Event(event)
}

fn embedded_and_user(user: Vec<String>) -> Vec<ThemeName> {
    config::EMBEDDED_THEMES
        .iter()
        .map(|name| (*name).to_string())
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
        domain::{ConfigError, ConfigFile, OptionCount, SettingId, ThemeName},
        update::Machine,
    };

    use crate::{
        config::{
            ConfigPaths,
            ConfigTiming,
            machine::{ConfigEffect, ConfigMessage, ConfigState, Published, Sighting},
            save_queue::{SavePatches, Saved},
            session::moment,
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
        ConfigState::new(&paths(theme), debounce())
    }

    fn debounce() -> Duration {
        ConfigTiming::default().save_debounce
    }

    fn save(state: ConfigState, patch: ConfigPatch, at: Instant) -> ConfigState {
        let (next, outputs) = state
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Save(patch),
                now: at,
            })
            .unwrap();
        assert!(outputs.is_empty());
        next
    }

    fn setting_id(field: config::AppearanceField) -> SettingId {
        config::APPEARANCE_ROWS[field as usize].custom.id
    }

    fn with_appearance_read(state: ConfigState) -> ConfigState {
        let (next, _) = state
            .transition(ConfigMessage::Read {
                file: ConfigFile::Appearance,
                text: None,
            })
            .unwrap();
        next
    }

    #[test]
    fn the_first_keymap_sighting_tells_nothing() {
        let (next, outputs) = driver(None)
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
        let (settled, _) = driver(None)
            .transition(ConfigMessage::Read {
                file: ConfigFile::Config,
                text: Some("[keymap]\nnext = \"x\"\n".to_string()),
            })
            .unwrap();

        let (_, outputs) = settled
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
        let (_, outputs) = driver(Some("noir"))
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
        let (_, outputs) = driver(None)
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
        let flushed = Saved {
            config: Some(Err(SaveError::Read {
                path: PathBuf::from("/config/config.toml"),
                source: std::io::Error::other("denied"),
            })),
            appearance: None,
        };

        let (_, outputs) = driver(None)
            .transition(ConfigMessage::Saved(flushed))
            .unwrap();

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
        let flushed = Saved {
            config: None,
            appearance: Some(Ok(Written {
                text: "[window]\nkey_hints = true\n".to_string(),
            })),
        };

        let (_, outputs) = driver(None)
            .transition(ConfigMessage::Saved(flushed))
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn select_theme_reads_the_theme_file() {
        let (_, outputs) = driver(None)
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::SelectTheme("noir".parse().unwrap()),
                now: moment(),
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
        let (_, outputs) = driver(None).transition(ConfigMessage::Stopping).unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn stopping_writes_a_save_whose_window_has_not_elapsed() {
        let patch = ConfigPatch::builder()
            .theme(ThemeName::from_static("dark"))
            .build();
        let pending = save(driver(None), patch.clone(), moment());

        let (next, outputs) = pending.transition(ConfigMessage::Stopping).unwrap();

        assert_eq!(
            outputs,
            vec![ConfigEffect::Save(SavePatches::Config(patch))]
        );
        assert_eq!(next.save_deadline(), None);
    }

    #[test]
    fn a_burst_of_saves_becomes_one_write_carrying_every_field() {
        let start = moment();
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
                Some(at + debounce()),
                "every new save in the burst pushes the trailing edge out"
            );
        }
        let last = start + Duration::from_millis(50);

        let (current, early) = current
            .transition(ConfigMessage::SaveDue { now: last })
            .unwrap();
        assert!(early.is_empty(), "the window has not elapsed yet");
        let (current, due) = current
            .transition(ConfigMessage::SaveDue {
                now: last + debounce(),
            })
            .unwrap();

        let expected = ConfigPatch::builder()
            .theme(ThemeName::from_static("noir"))
            .volume(Percent::clamped(50))
            .build();
        assert_eq!(due, vec![ConfigEffect::Save(SavePatches::Config(expected))]);
        assert_eq!(current.save_deadline(), None);
    }

    #[test]
    fn a_setting_publishes_the_patched_appearance_and_queues_a_save() {
        let seeded = with_appearance_read(driver(None));
        let option = OptionCount::new(2).unwrap().index(1).unwrap();

        let patch = config::appearance_patch(
            setting_id(config::AppearanceField::CoverBrackets),
            option,
        )
        .unwrap();
        let expected = seeded.appearance_file.patched(patch);
        let at = moment();

        let (next, outputs) = seeded
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Setting {
                    id: setting_id(config::AppearanceField::CoverBrackets),
                    option,
                },
                now: at,
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigEffect::Changed(Published::Appearance(_))]
        ));
        assert_eq!(next.save_deadline(), Some(at + debounce()));
        assert_eq!(*next.appearance_file, expected);
    }

    #[test]
    fn an_unknown_setting_publishes_nothing() {
        let seeded = with_appearance_read(driver(None));

        let (_, outputs) = seeded
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Setting {
                    id: SettingId::new(u16::MAX),
                    option: OptionCount::new(1).unwrap().index(0).unwrap(),
                },
                now: moment(),
            })
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn a_setting_before_the_first_appearance_read_is_dropped() {
        let (next, outputs) = driver(None)
            .transition(ConfigMessage::Command {
                cmd: ConfigCmd::Setting {
                    id: setting_id(config::AppearanceField::CoverBrackets),
                    option: OptionCount::new(2).unwrap().index(1).unwrap(),
                },
                now: moment(),
            })
            .unwrap();

        assert!(outputs.is_empty());
        assert_eq!(next.save_deadline(), None);
    }

    #[test]
    fn a_command_save_queues_the_patch() {
        let at = moment();

        let next = save(driver(None), ConfigPatch::builder().build(), at);

        assert_eq!(next.save_deadline(), Some(at + debounce()));
    }
}
