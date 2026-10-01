use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
    time::Instant,
};

use config::{AppearanceFile, ThemeFile};
use crossbeam_channel::{Receiver, Select, unbounded};
use kernel::{
    ConfigCmd,
    ConfigEvent,
    DriverMessage,
    Outbox,
    SendError,
    domain::{ConfigError, ConfigFile, Driver},
    update::Machine,
};
use notify::RecommendedWatcher;

use crate::{
    config::{
        disk::{list_theme_names, read},
        driver::{ConfigParts, Outbound},
        machine::{ConfigEffect, ConfigMessage, ConfigState, Published},
        save_queue::SavePatches,
        write::{SaveResult, save_appearance, save_config},
    },
    latest::LatestSender,
    sender::DriverSender,
    watcher::{Watcher, config_directory, recommended, watch_if_present},
};

enum Wake {
    Command(ConfigCmd),
    FilesystemChange,
    FilesystemEventsLost,
    SaveDeadlineElapsed,
    Stopped,
}

pub(crate) struct Watch<W> {
    pub(crate) watcher: W,
    pub(crate) events: Receiver<notify::Result<notify::Event>>,
}

impl Watch<Option<RecommendedWatcher>> {
    pub(crate) fn recommended(outbox: &DriverSender<ConfigEvent>) -> Self {
        let (events, receiver) = unbounded();
        let watcher = recommended(events, |error| {
            match outbox.send(watch_failure(&error.to_string())) {
                Ok(()) | Err(SendError::Full | SendError::Closed) => {}
            }
        });
        Self {
            watcher,
            events: receiver,
        }
    }
}

pub(crate) struct ConfigLoop<'a, W> {
    driver: ConfigState,
    watcher: W,
    filesystem_events: Receiver<notify::Result<notify::Event>>,
    config_path: PathBuf,
    appearance_path: PathBuf,
    outbox: &'a DriverSender<ConfigEvent>,
    theme: &'a LatestSender<ThemeFile>,
    appearance: &'a LatestSender<AppearanceFile>,
}

impl<'a, W: Watcher> ConfigLoop<'a, W> {
    pub(crate) fn new(
        parts: &ConfigParts,
        outbound: &Outbound<'a>,
        watching: Watch<W>,
    ) -> Self {
        let paths = &parts.paths;
        let mut loaded = Self {
            driver: ConfigState::new(paths, parts.save_debounce),
            watcher: watching.watcher,
            filesystem_events: watching.events,
            config_path: paths.config.clone(),
            appearance_path: paths.appearance.clone(),
            outbox: outbound.outbox,
            theme: outbound.theme,
            appearance: outbound.appearance,
        };
        loaded.mount(&config_directory(paths));
        loaded
    }

    fn mount(&mut self, directory: &Path) {
        if let Err(error) = watch_if_present(&mut self.watcher, directory) {
            let _closed = self.emit(watch_failure(&error.to_string()));
        }
    }

    pub(crate) fn run(mut self, inbox: &Receiver<ConfigCmd>) {
        if self.feed(ConfigMessage::FilesChanged).is_break() {
            return;
        }
        loop {
            match self.wait(inbox) {
                Wake::Command(command) => {
                    let input = ConfigMessage::Command {
                        cmd: command,
                        now: Instant::now(),
                    };
                    if self.feed(input).is_break() {
                        return;
                    }
                }
                Wake::FilesystemChange => {
                    if self.feed(ConfigMessage::FilesChanged).is_break() {
                        return;
                    }
                }
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                }
                Wake::SaveDeadlineElapsed => {
                    let due = ConfigMessage::SaveDue {
                        now: Instant::now(),
                    };
                    if self.feed(due).is_break() {
                        return;
                    }
                }
                Wake::Stopped => {
                    let _flushed = self.feed(ConfigMessage::Stopping);
                    return;
                }
            }
        }
    }

    fn wait(&self, inbox: &Receiver<ConfigCmd>) -> Wake {
        let mut select = Select::new();
        let command_index = select.recv(inbox);
        let filesystem_index = select.recv(&self.filesystem_events);
        let selected = match self.driver.save_deadline() {
            Some(deadline) => select.select_deadline(deadline),
            None => Ok(select.select()),
        };
        let Ok(operation) = selected else {
            return Wake::SaveDeadlineElapsed;
        };
        if operation.index() == command_index {
            return operation.recv(inbox).map_or(Wake::Stopped, Wake::Command);
        }
        if operation.index() == filesystem_index {
            return operation
                .recv(&self.filesystem_events)
                .map_or(Wake::FilesystemEventsLost, |_event| Wake::FilesystemChange);
        }
        Wake::Stopped
    }

    fn feed(&mut self, input: ConfigMessage) -> ControlFlow<()> {
        let label: &'static str = (&input).into();
        match self.driver.update(input) {
            Ok(outputs) => self.act_all(outputs),
            Err(_) => self.reject(label),
        }
    }

    fn act_all(&mut self, outputs: Vec<ConfigEffect>) -> ControlFlow<()> {
        outputs.into_iter().try_for_each(|output| self.act(output))
    }

    fn reject(&self, input: &'static str) -> ControlFlow<()> {
        let rejected = DriverMessage::Rejected { input };
        match self.outbox.report(Driver::Config, rejected) {
            Err(SendError::Closed) => ControlFlow::Break(()),
            Ok(()) | Err(SendError::Full) => ControlFlow::Continue(()),
        }
    }

    fn act(&mut self, output: ConfigEffect) -> ControlFlow<()> {
        match output {
            ConfigEffect::Read { file, path } => self.read(file, &path),
            ConfigEffect::List(dir) => self.list(&dir),
            ConfigEffect::Save(patches) => {
                let saved = self.save(patches);
                self.feed(saved)
            }
            ConfigEffect::Changed(published) => {
                self.publish(published);
                ControlFlow::Continue(())
            }
            ConfigEffect::Event(event) => self.emit(event),
        }
    }

    fn read(&mut self, file: ConfigFile, path: &Path) -> ControlFlow<()> {
        self.feed(read(file, path))
    }

    fn save(&self, patches: SavePatches) -> ConfigMessage {
        let config: Option<SaveResult> = patches
            .config
            .map(|patch| save_config(&self.config_path, patch));
        let appearance: Option<SaveResult> = patches
            .appearance
            .map(|patch| save_appearance(&self.appearance_path, patch));
        ConfigMessage::Saved { config, appearance }
    }

    fn list(&mut self, dir: &Path) -> ControlFlow<()> {
        let listing = list_theme_names(dir);
        self.feed(ConfigMessage::Listed(listing))
    }

    fn publish(&self, published: Published) {
        match published {
            Published::Theme(file) => self.theme.publish(file),
            Published::Appearance(file) => self.appearance.publish(file),
        }
    }

    fn emit(&self, event: ConfigEvent) -> ControlFlow<()> {
        match self.outbox.send(event) {
            Err(SendError::Closed) => ControlFlow::Break(()),
            Ok(()) | Err(SendError::Full) => ControlFlow::Continue(()),
        }
    }
}

fn watch_failure(reason: &str) -> ConfigEvent {
    ConfigEvent::Error(ConfigError::Watch {
        detail: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use config::AppearanceField;
    use crossbeam_channel::{SendError, unbounded};
    use kernel::{
        ConfigCmd,
        ConfigPatch,
        DriverMessage,
        Message,
        domain::{ConfigFile, Driver, ThemeName},
    };

    use crate::{
        config::{
            ConfigPaths,
            driver::{ConfigParts, Outbound, spawn as spawn_config},
            fixtures::{drain, paths},
            machine::ConfigMessage,
            save_queue::SavePatches,
            session::{ConfigLoop, Wake, Watch},
        },
        latest::{LatestSenders, latest_channels},
        sender::{DriverSender, FullEdge},
        watcher::FakeWatch,
    };

    const HAND_EDITED_THEME: &str = "name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    const TEST_DEBOUNCE: Duration = Duration::from_millis(20);

    fn parts(paths: ConfigPaths, writers: LatestSenders) -> ConfigParts {
        ConfigParts {
            paths,
            save_debounce: TEST_DEBOUNCE,
            theme: writers.theme,
            appearance: writers.appearance,
        }
    }

    #[test]
    fn a_fake_theme_change_publishes_the_theme() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, _messages) = unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, cells, _doorbell) = latest_channels();
        let parts = parts(paths(&directory), writers);
        let outbound = Outbound {
            outbox: &outbox,
            theme: &parts.theme,
            appearance: &parts.appearance,
        };
        let (events, receiver) = unbounded();
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: receiver,
        };
        let mut session = ConfigLoop::new(&parts, &outbound, watching);
        assert_eq!(
            session.watcher.watched,
            vec![directory.path().to_path_buf()]
        );
        let _halt = session.feed(ConfigMessage::FilesChanged);
        let embedded = cells.theme.take();
        assert_eq!(
            embedded.map(|theme| theme.name.clone()),
            Some(ThemeName::from_static("noir"))
        );

        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        std::fs::write(directory.path().join("themes/noir.toml"), HAND_EDITED_THEME)
            .unwrap();
        events.send(Ok(notify::Event::default())).unwrap();
        let (_commands, inbox) = unbounded();
        assert!(matches!(session.wait(&inbox), Wake::FilesystemChange));
        let _republished = session.feed(ConfigMessage::FilesChanged);

        let edited = cells.theme.take();
        assert!(edited.is_some(), "a changed theme file must be published");
    }

    #[test]
    fn a_rejected_input_is_reported_to_the_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let mut unselected = paths(&directory);
        unselected.theme = None;
        let parts = parts(unselected, writers);
        let outbound = Outbound {
            outbox: &outbox,
            theme: &parts.theme,
            appearance: &parts.appearance,
        };
        let (_events, receiver) = unbounded();
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: receiver,
        };
        let mut session = ConfigLoop::new(&parts, &outbound, watching);

        let halt = session.feed(ConfigMessage::Read {
            file: ConfigFile::Theme,
            text: None,
        });

        assert!(halt.is_continue());
        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver {
                driver: Driver::Config,
                event: DriverMessage::Rejected { input: "Read" }
            }]
        );
        assert!(session.feed(ConfigMessage::FilesChanged).is_continue());
    }

    #[test]
    fn saving_lands_a_config_patch_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, _messages) = unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let parts = parts(paths(&directory), writers);
        let outbound = Outbound {
            outbox: &outbox,
            theme: &parts.theme,
            appearance: &parts.appearance,
        };
        let (_events, receiver) = unbounded();
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: receiver,
        };
        let session = ConfigLoop::new(&parts, &outbound, watching);
        let patches = SavePatches {
            config: Some(
                ConfigPatch::builder()
                    .theme(ThemeName::from_static("dark"))
                    .build(),
            ),
            appearance: None,
        };

        let saved = session.save(patches);

        let text =
            std::fs::read_to_string(directory.path().join("config.toml")).unwrap();
        assert!(text.contains("theme = \"dark\""));
        assert!(matches!(
            saved,
            ConfigMessage::Saved {
                config: Some(Ok(_)),
                appearance: None
            }
        ));
    }

    #[test]
    fn a_closed_inbox_ends_the_config_driver() {
        let directory = tempfile::tempdir().unwrap();
        let (inbox, messages) = unbounded();
        let (writers, _cells, doorbell) = latest_channels();
        let thread = spawn_config(parts(paths(&directory), writers), &inbox).unwrap();

        drain(&doorbell);
        drain(&messages);

        let row = config::APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == AppearanceField::KeyHints)
            .unwrap();
        thread
            .commands
            .send(ConfigCmd::Setting {
                id: row.custom.id,
                option: row.custom.control.count().index(1).unwrap(),
            })
            .unwrap();
        thread
            .commands
            .send(ConfigCmd::Save(ConfigPatch::builder().build()))
            .unwrap();
        drop(messages);
        drop(thread.commands);

        let report = thread.handle.join().unwrap();

        assert_eq!(
            report,
            Err(SendError(Message::Driver {
                driver: Driver::Config,
                event: DriverMessage::Stopped
            }))
        );
        let content =
            std::fs::read_to_string(directory.path().join("sifr-ui.toml")).unwrap();
        assert!(
            content.contains("key_hints"),
            "the pending save must land on disk"
        );
    }
}
