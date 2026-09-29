use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use config::{AppearanceFile, ThemeFile};
use kernel::{
    ConfigFact,
    domain::{
        ConfigFailure,
        ConfigFile,
        ConfigSource,
        OptionIndex,
        SettingId,
        ThemeName,
    },
    update::{Machine, Rejected},
};
use strum::IntoStaticStr;

use crate::{
    config::{
        ConfigPaths,
        coalesce::{Flushed, SavePatches, SaveQueue},
        disk::Listing,
        reload::{appearance_reload, keymap_reload, theme_reload},
        watch::{
            ConfigChange,
            ConfigIo,
            ConfigWatch,
            ConfigWatchMessage,
            ConfigWatchRejection,
            WatchedFile,
        },
        write::Written,
    },
    error::SaveError,
    interpret::ConfigCommand,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum KeysSighting {
    #[default]
    First,
    Repeat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AppearanceSighting {
    #[default]
    Unseen,
    Seen,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ConfigDriver {
    watch: Box<ConfigWatch>,
    keys: KeysSighting,
    appearance: Box<AppearanceFile>,
    appearance_seen: AppearanceSighting,
    saves: Box<SaveQueue>,
}

#[derive(Debug, IntoStaticStr)]
pub(crate) enum ConfigInput {
    Command(ConfigCommand, Instant),
    FilesChanged,
    Read {
        file: WatchedFile,
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
    Saved(Flushed),
    Stopping,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Published {
    Theme(ThemeFile),
    Appearance(AppearanceFile),
}

#[derive(Debug, PartialEq)]
pub(crate) enum ConfigOutput {
    Read { file: WatchedFile, path: PathBuf },
    List(PathBuf),
    Write(SavePatches),
    Publish(Published),
    Tell(ConfigFact),
}

#[derive(Clone, Copy)]
struct SaveTarget {
    file: ConfigFile,
    wrote: fn(String) -> ConfigWatchMessage,
}

impl ConfigDriver {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths, debounce: Duration) -> Self {
        Self {
            watch: Box::new(ConfigWatch::new(paths)),
            keys: KeysSighting::First,
            appearance: Box::default(),
            appearance_seen: AppearanceSighting::Unseen,
            saves: Box::new(SaveQueue::new(debounce)),
        }
    }

    #[must_use]
    pub(crate) fn save_deadline(&self) -> Option<Instant> {
        self.saves.next_deadline()
    }
}

type Step = Result<(ConfigDriver, Vec<ConfigOutput>), Rejected<ConfigDriver>>;

impl Machine for ConfigDriver {
    type Message = ConfigInput;
    type Rejection = ConfigWatchRejection;
    type Effect = Vec<ConfigOutput>;

    fn transition(mut self, input: ConfigInput) -> Step {
        match input {
            ConfigInput::Command(ConfigCommand::Save(patch), now) => {
                self.saves.queue(now, patch);
                Ok((self, Vec::new()))
            }
            ConfigInput::Command(ConfigCommand::SelectTheme(name), _) => {
                self.drive(ConfigWatchMessage::SelectTheme(name))
            }
            ConfigInput::Command(ConfigCommand::Setting { id, option }, now) => {
                Ok(self.setting((id, option), now))
            }
            ConfigInput::FilesChanged => Ok(self.poll_everything()),
            ConfigInput::Read { file, text } => {
                self.drive(ConfigWatchMessage::Observed { file, text })
            }
            ConfigInput::Unreadable { file, detail } => {
                self.drive(ConfigWatchMessage::Unreadable { file, detail })
            }
            ConfigInput::Listed(Listing::Names(names)) => {
                self.drive(ConfigWatchMessage::Listed(names))
            }
            ConfigInput::Listed(Listing::Unreadable(detail)) => {
                self.drive(ConfigWatchMessage::Unreadable {
                    file: ConfigFile::ThemeDirectory,
                    detail,
                })
            }
            ConfigInput::SaveDue { now } => {
                let writes = self.saves.take_due(now);
                Ok((self, write_outputs(writes)))
            }
            ConfigInput::Saved(flushed) => Ok(self.saved(flushed)),
            ConfigInput::Stopping => {
                let writes = self.saves.take_all();
                Ok((self, write_outputs(writes)))
            }
        }
    }
}

impl ConfigDriver {
    fn drive(mut self, message: ConfigWatchMessage) -> Step {
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

    fn react(&mut self, io: ConfigIo) -> Vec<ConfigOutput> {
        match io {
            ConfigIo::Nothing => Vec::new(),
            ConfigIo::Read { file, path } => vec![ConfigOutput::Read { file, path }],
            ConfigIo::List(dir) => vec![ConfigOutput::List(dir)],
            ConfigIo::Send(change) => self.changed(change),
        }
    }

    fn changed(&mut self, change: ConfigChange) -> Vec<ConfigOutput> {
        match change {
            ConfigChange::Appearance(text) => self.appearance_changed(text.as_deref()),
            ConfigChange::Keymap(text) => self.keymap_changed(text.as_deref()),
            ConfigChange::Theme { name, text } => theme_changed(&name, text.as_deref()),
            ConfigChange::Themes(names) => {
                vec![tell(ConfigFact::ThemesLoaded(embedded_and_user(names)))]
            }
            ConfigChange::Unreadable { file, detail } => {
                vec![tell(ConfigFact::Failed(ConfigFailure::Unreadable {
                    file,
                    detail,
                }))]
            }
        }
    }

    fn setting(
        mut self,
        (id, option): (SettingId, OptionIndex),
        now: Instant,
    ) -> (Self, Vec<ConfigOutput>) {
        if self.appearance_seen == AppearanceSighting::Unseen {
            return (self, Vec::new());
        }
        match config::appearance_patch(id, option) {
            Ok(patch) => {
                *self.appearance = self.appearance.patched(patch);
                self.saves.queue_appearance(now, patch);
                let published = (*self.appearance).clone();
                let outputs =
                    vec![ConfigOutput::Publish(Published::Appearance(published))];
                (self, outputs)
            }
            Err(_) => (self, Vec::new()),
        }
    }

    fn keymap_changed(&mut self, text: Option<&str>) -> Vec<ConfigOutput> {
        match keymap_reload(text) {
            Ok(parsed) => {
                let sighting = std::mem::replace(&mut self.keys, KeysSighting::Repeat);
                if sighting == KeysSighting::First {
                    return Vec::new();
                }
                let mut outputs =
                    vec![tell(ConfigFact::KeymapReloaded(Box::new(parsed.keymap)))];
                if let Some(music_dir) = parsed.music_dir {
                    outputs.push(tell(ConfigFact::MusicDirReloaded(music_dir)));
                }
                outputs
            }
            Err(error) => vec![source_failed(ConfigSource::Keymap, error.to_string())],
        }
    }

    fn poll_everything(mut self) -> (Self, Vec<ConfigOutput>) {
        let mut outputs = Vec::new();
        for message in [
            ConfigWatchMessage::Poll(WatchedFile::Appearance),
            ConfigWatchMessage::Poll(WatchedFile::Keys),
            ConfigWatchMessage::Poll(WatchedFile::Theme),
            ConfigWatchMessage::PollThemes,
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

    fn saved(mut self, flushed: Flushed) -> (Self, Vec<ConfigOutput>) {
        let mut outputs = Vec::new();
        if let Some(result) = flushed.config {
            let target = SaveTarget {
                file: ConfigFile::Keymap,
                wrote: ConfigWatchMessage::WroteKeys,
            };
            let (next, mut more) = self.note_save(result, target);
            self = next;
            outputs.append(&mut more);
        }
        if let Some(result) = flushed.appearance {
            let target = SaveTarget {
                file: ConfigFile::Appearance,
                wrote: ConfigWatchMessage::WroteAppearance,
            };
            let (next, mut more) = self.note_save(result, target);
            self = next;
            outputs.append(&mut more);
        }
        (self, outputs)
    }

    fn note_save(
        self,
        result: Result<Written, SaveError>,
        target: SaveTarget,
    ) -> (Self, Vec<ConfigOutput>) {
        match result {
            Ok(written) => match self.drive((target.wrote)(written.text)) {
                Ok(pair) => pair,
                Err(rejected) => (rejected.state, Vec::new()),
            },
            Err(error) => (
                self,
                vec![tell(ConfigFact::Failed(ConfigFailure::Save {
                    file: target.file,
                    detail: error.to_string(),
                }))],
            ),
        }
    }
}

impl ConfigDriver {
    fn appearance_changed(&mut self, text: Option<&str>) -> Vec<ConfigOutput> {
        match appearance_reload(text) {
            Ok(file) => {
                let rows = config::custom_rows(&file);
                *self.appearance = file.clone();
                self.appearance_seen = AppearanceSighting::Seen;
                vec![
                    ConfigOutput::Publish(Published::Appearance(file)),
                    tell(ConfigFact::CustomRowsReloaded(rows)),
                    source_recovered(ConfigSource::Appearance),
                ]
            }
            Err(error) => {
                vec![source_failed(ConfigSource::Appearance, error.to_string())]
            }
        }
    }
}

fn theme_changed(name: &str, text: Option<&str>) -> Vec<ConfigOutput> {
    match theme_reload(name, text) {
        Ok(file) => {
            let mut outputs = vec![ConfigOutput::Publish(Published::Theme(file))];
            if let Ok(theme_name) = ThemeName::new(name.to_string()) {
                outputs.push(tell(ConfigFact::ThemeReloaded(theme_name)));
            }
            outputs.push(source_recovered(ConfigSource::Theme));
            outputs
        }
        Err(error) => vec![source_failed(ConfigSource::Theme, error.to_string())],
    }
}

fn write_outputs(writes: Option<SavePatches>) -> Vec<ConfigOutput> {
    writes.map(ConfigOutput::Write).into_iter().collect()
}

fn source_recovered(source: ConfigSource) -> ConfigOutput {
    tell(ConfigFact::SourceRecovered(source))
}

fn source_failed(source: ConfigSource, text: String) -> ConfigOutput {
    tell(ConfigFact::SourceFailed { source, text })
}

fn tell(fact: ConfigFact) -> ConfigOutput {
    ConfigOutput::Tell(fact)
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
        ConfigPatch,
        Percent,
        domain::{
            ConfigFailure,
            ConfigFile,
            ConfigSource,
            OptionCount,
            SettingId,
            ThemeName,
        },
        update::Machine,
    };

    use crate::{
        config::{
            ConfigPaths,
            ConfigTiming,
            coalesce::{Flushed, SavePatches},
            machine::{
                ConfigDriver,
                ConfigInput,
                ConfigOutput,
                KeysSighting,
                Published,
            },
            session::moment,
            watch::WatchedFile,
            write::Written,
        },
        error::SaveError,
        interpret::ConfigCommand,
    };

    const NOIR_THEME: &str = "name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    fn paths(theme: Option<&str>) -> ConfigPaths {
        ConfigPaths {
            config: Some(PathBuf::from("/config/config.toml")),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            theme: theme.map(str::to_string),
            seen: crate::config::SeenTexts::default(),
        }
    }

    fn driver(theme: Option<&str>) -> ConfigDriver {
        ConfigDriver::new(&paths(theme), debounce())
    }

    fn debounce() -> Duration {
        ConfigTiming::default().save_debounce
    }

    fn save(driver: ConfigDriver, patch: ConfigPatch, at: Instant) -> ConfigDriver {
        let (next, outputs) = driver
            .transition(ConfigInput::Command(ConfigCommand::Save(patch), at))
            .unwrap();
        assert!(outputs.is_empty());
        next
    }

    fn setting_id(field: config::AppearanceField) -> SettingId {
        config::APPEARANCE_ROWS[field as usize].spec.id
    }

    fn with_appearance_read(driver: ConfigDriver) -> ConfigDriver {
        let (next, _) = driver
            .transition(ConfigInput::Read {
                file: WatchedFile::Appearance,
                text: None,
            })
            .unwrap();
        next
    }

    #[test]
    fn the_first_keymap_sighting_tells_nothing() {
        let (next, outputs) = driver(None)
            .transition(ConfigInput::Read {
                file: WatchedFile::Keys,
                text: Some("[keymap]\nnext = \"x\"\n".to_string()),
            })
            .unwrap();

        assert!(outputs.is_empty());
        assert_eq!(next.keys, KeysSighting::Repeat);
    }

    #[test]
    fn a_second_keymap_sighting_tells_keymap_reloaded() {
        let (settled, _) = driver(None)
            .transition(ConfigInput::Read {
                file: WatchedFile::Keys,
                text: Some("[keymap]\nnext = \"x\"\n".to_string()),
            })
            .unwrap();

        let (_, outputs) = settled
            .transition(ConfigInput::Read {
                file: WatchedFile::Keys,
                text: Some("[keymap]\nnext = \"y\"\n".to_string()),
            })
            .unwrap();

        assert!(outputs.iter().any(|output| matches!(
            output,
            ConfigOutput::Tell(kernel::ConfigFact::KeymapReloaded(_))
        )));
    }

    #[test]
    fn a_changed_theme_publishes_then_tells_theme_reloaded() {
        let (_, outputs) = driver(Some("noir"))
            .transition(ConfigInput::Read {
                file: WatchedFile::Theme,
                text: Some(NOIR_THEME.to_string()),
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                ConfigOutput::Publish(Published::Theme(_)),
                ConfigOutput::Tell(kernel::ConfigFact::ThemeReloaded(_)),
                ConfigOutput::Tell(kernel::ConfigFact::SourceRecovered(
                    ConfigSource::Theme
                )),
            ]
        ));
    }

    #[test]
    fn an_unparsable_appearance_tells_config_failed() {
        let (_, outputs) = driver(None)
            .transition(ConfigInput::Read {
                file: WatchedFile::Appearance,
                text: Some("[cover\nnot toml".to_string()),
            })
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigOutput::Tell(kernel::ConfigFact::SourceFailed {
                source: ConfigSource::Appearance,
                ..
            })]
        ));
    }

    #[test]
    fn a_failed_save_tells_save_failed() {
        let flushed = Flushed {
            config: Some(Err(SaveError::NoConfigDirectory)),
            appearance: None,
        };

        let (_, outputs) = driver(None)
            .transition(ConfigInput::Saved(flushed))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigOutput::Tell(kernel::ConfigFact::Failed(
                ConfigFailure::Save {
                    file: ConfigFile::Keymap,
                    ..
                }
            ))]
        ));
    }

    #[test]
    fn a_successful_appearance_save_tells_nothing() {
        let flushed = Flushed {
            config: None,
            appearance: Some(Ok(Written {
                text: "[window]\nkey_hints = true\n".to_string(),
            })),
        };

        let (_, outputs) = driver(None)
            .transition(ConfigInput::Saved(flushed))
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn select_theme_reads_the_theme_file() {
        let (_, outputs) = driver(None)
            .transition(ConfigInput::Command(
                ConfigCommand::SelectTheme("noir".to_string()),
                moment(),
            ))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigOutput::Read {
                file: WatchedFile::Theme,
                ..
            }]
        ));
    }

    #[test]
    fn stopping_with_nothing_pending_writes_nothing() {
        let (_, outputs) = driver(None).transition(ConfigInput::Stopping).unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn stopping_writes_a_save_whose_window_has_not_elapsed() {
        let patch = ConfigPatch::builder()
            .theme(ThemeName::from_static("dark"))
            .build();
        let pending = save(driver(None), patch.clone(), moment());

        let (next, outputs) = pending.transition(ConfigInput::Stopping).unwrap();

        assert_eq!(
            outputs,
            vec![ConfigOutput::Write(SavePatches {
                config: Some(patch),
                appearance: None,
            })]
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
            .transition(ConfigInput::SaveDue { now: last })
            .unwrap();
        assert!(early.is_empty(), "the window has not elapsed yet");
        let (current, due) = current
            .transition(ConfigInput::SaveDue {
                now: last + debounce(),
            })
            .unwrap();

        let expected = ConfigPatch::builder()
            .theme(ThemeName::from_static("noir"))
            .volume(Percent::clamped(50))
            .build();
        assert_eq!(
            due,
            vec![ConfigOutput::Write(SavePatches {
                config: Some(expected),
                appearance: None,
            })]
        );
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
        let expected = seeded.appearance.patched(patch);
        let at = moment();

        let (next, outputs) = seeded
            .transition(ConfigInput::Command(
                ConfigCommand::Setting {
                    id: setting_id(config::AppearanceField::CoverBrackets),
                    option,
                },
                at,
            ))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [ConfigOutput::Publish(Published::Appearance(_))]
        ));
        assert_eq!(next.save_deadline(), Some(at + debounce()));
        assert_eq!(*next.appearance, expected);
    }

    #[test]
    fn an_unknown_setting_publishes_nothing() {
        let seeded = with_appearance_read(driver(None));

        let (_, outputs) = seeded
            .transition(ConfigInput::Command(
                ConfigCommand::Setting {
                    id: SettingId::new(u16::MAX),
                    option: OptionCount::new(1).unwrap().index(0).unwrap(),
                },
                moment(),
            ))
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn a_setting_before_the_first_appearance_read_is_dropped() {
        let (next, outputs) = driver(None)
            .transition(ConfigInput::Command(
                ConfigCommand::Setting {
                    id: setting_id(config::AppearanceField::CoverBrackets),
                    option: OptionCount::new(2).unwrap().index(1).unwrap(),
                },
                moment(),
            ))
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
