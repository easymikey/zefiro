use std::{path::Path, time::Instant};

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
    cells::LatestSender,
    config::{
        ConfigPaths,
        ConfigTiming,
        disk::{list_theme_names, read},
        driver::Outbound,
        machine::{ConfigEffect, ConfigMessage, ConfigState, Published},
        save_queue::SavePaths,
        watch::WatchMessage,
    },
    sender::DriverSender,
    watcher::{Watcher, config_directory, watch_if_present},
};

enum Wake {
    Command(ConfigCmd),
    FilesystemChange,
    FilesystemEventsLost,
    SaveDeadlineElapsed,
    Stopped,
}

enum Halt {
    Continue,
    Stop,
}

pub(crate) struct Watching<W> {
    pub(crate) watcher: W,
    pub(crate) events: Receiver<notify::Result<notify::Event>>,
}

impl Watching<Option<RecommendedWatcher>> {
    pub(crate) fn recommended(mailbox: &DriverSender<ConfigEvent>) -> Self {
        let (events, receiver) = unbounded();
        let watcher = notify::recommended_watcher(events).map_or_else(
            |error| {
                match mailbox.send(watch_failure(&error.to_string())) {
                    Ok(()) | Err(SendError::Full | SendError::Closed) => {}
                }
                None
            },
            Some,
        );
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
    saving: SavePaths,
    mailbox: &'a DriverSender<ConfigEvent>,
    theme: &'a LatestSender<ThemeFile>,
    appearance: &'a LatestSender<AppearanceFile>,
}

impl<'a, W: Watcher> ConfigLoop<'a, W> {
    pub(crate) fn new(
        config: (&ConfigPaths, ConfigTiming),
        outbound: &Outbound<'a>,
        watching: Watching<W>,
    ) -> Self {
        let (paths, timing) = config;
        let mut loaded = Self {
            driver: ConfigState::new(paths, timing.save_debounce),
            watcher: watching.watcher,
            filesystem_events: watching.events,
            saving: SavePaths::new(paths),
            mailbox: outbound.mailbox,
            theme: outbound.theme,
            appearance: outbound.appearance,
        };
        loaded.mount(&config_directory(paths));
        loaded
    }

    fn mount(&mut self, directory: &Path) {
        if let Err(error) = watch_if_present(&mut self.watcher, directory) {
            self.emit(watch_failure(&error.to_string()));
        }
    }

    pub(crate) fn run(mut self, inbox: &Receiver<ConfigCmd>) {
        if let Halt::Stop = self.feed(ConfigMessage::FilesChanged) {
            return;
        }
        loop {
            match self.wait(inbox) {
                Wake::Command(command) => {
                    let input = ConfigMessage::Command {
                        cmd: command,
                        now: moment(),
                    };
                    if let Halt::Stop = self.feed(input) {
                        return;
                    }
                }
                Wake::FilesystemChange => {
                    if let Halt::Stop = self.feed(ConfigMessage::FilesChanged) {
                        return;
                    }
                }
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                }
                Wake::SaveDeadlineElapsed => {
                    let due = ConfigMessage::SaveDue { now: moment() };
                    if let Halt::Stop = self.feed(due) {
                        return;
                    }
                }
                Wake::Stopped => {
                    self.feed(ConfigMessage::Stopping);
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

    fn feed(&mut self, input: ConfigMessage) -> Halt {
        let label: &'static str = (&input).into();
        match self.driver.update(input) {
            Ok(outputs) => self.act_all(outputs),
            Err(_) => self.reject(label),
        }
    }

    fn act_all(&mut self, outputs: Vec<ConfigEffect>) -> Halt {
        for output in outputs {
            if let Halt::Stop = self.act(output) {
                return Halt::Stop;
            }
        }
        Halt::Continue
    }

    fn reject(&self, input: &'static str) -> Halt {
        let rejected = DriverMessage::Rejected { input };
        match self.mailbox.report(Driver::Config, rejected) {
            Err(SendError::Closed) => Halt::Stop,
            Ok(()) | Err(SendError::Full) => Halt::Continue,
        }
    }

    fn act(&mut self, output: ConfigEffect) -> Halt {
        match output {
            ConfigEffect::Read { file, path } => self.read(file, &path),
            ConfigEffect::List(dir) => self.list(&dir),
            ConfigEffect::Save(patches) => {
                let flushed = self.saving.write(patches);
                self.feed(ConfigMessage::Saved(flushed))
            }
            ConfigEffect::Changed(published) => {
                self.publish(published);
                Halt::Continue
            }
            ConfigEffect::Event(event) => self.emit(event),
        }
    }

    fn read(&mut self, file: ConfigFile, path: &Path) -> Halt {
        match read(file, path) {
            WatchMessage::Observed { file, text } => {
                self.feed(ConfigMessage::Read { file, text })
            }
            WatchMessage::Unreadable { file, detail } => {
                self.feed(ConfigMessage::Unreadable { file, detail })
            }
            WatchMessage::Poll(_)
            | WatchMessage::PollThemes
            | WatchMessage::Listed(_)
            | WatchMessage::ThemesUnreadable(_)
            | WatchMessage::SelectTheme(_)
            | WatchMessage::WroteAppearance(_)
            | WatchMessage::WroteConfig(_) => Halt::Continue,
        }
    }

    fn list(&mut self, dir: &Path) -> Halt {
        let listing = list_theme_names(dir);
        self.feed(ConfigMessage::Listed(listing))
    }

    fn publish(&self, published: Published) {
        match published {
            Published::Theme(file) => self.theme.publish(file),
            Published::Appearance(file) => self.appearance.publish(file),
        }
    }

    fn emit(&self, event: ConfigEvent) -> Halt {
        match self.mailbox.send(event) {
            Err(SendError::Closed) => Halt::Stop,
            Ok(()) | Err(SendError::Full) => Halt::Continue,
        }
    }
}

pub(crate) fn moment() -> Instant {
    Instant::now()
}

fn watch_failure(reason: &str) -> ConfigEvent {
    ConfigEvent::Error(ConfigError::Watch {
        detail: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    use config::AppearanceField;
    use crossbeam_channel::{Receiver, SendError, unbounded};
    use kernel::{
        ConfigCmd,
        ConfigPatch,
        DriverMessage,
        Message,
        domain::{ConfigFile, Driver},
    };

    use crate::{
        cells::cells,
        config::{
            ConfigPaths,
            ConfigTiming,
            driver::{ConfigParts, Outbound, spawn as spawn_config},
            machine::ConfigMessage,
            session::{ConfigLoop, Halt, Wake, Watching},
        },
        sender::{DriverSender, FullEdge},
        watcher::Watcher,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    const HAND_EDITED_THEME: &str = "name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n";

    #[derive(Default)]
    struct FakeWatch {
        watched: Vec<PathBuf>,
    }

    impl Watcher for FakeWatch {
        fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.retain(|watched| watched != path);
            Ok(())
        }
    }

    fn paths(directory: &tempfile::TempDir) -> ConfigPaths {
        ConfigPaths {
            config: directory.path().join("config.toml"),
            appearance: directory.path().join("sifr-ui.toml"),
            themes: directory.path().join("themes"),
            theme: Some("noir".to_string()),
            seen: crate::config::SeenTexts::default(),
        }
    }

    fn timing() -> ConfigTiming {
        ConfigTiming {
            save_debounce: Duration::from_millis(20),
        }
    }

    fn drain<T>(receiver: &Receiver<T>) {
        receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        while receiver.recv_timeout(SETTLE_TIMEOUT).is_ok() {}
    }

    #[test]
    fn a_fake_theme_change_publishes_the_theme() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, _messages) = unbounded::<Message>();
        let mailbox = DriverSender::new(sender, FullEdge::default());
        let (writers, cells, _doorbell) = cells();
        let outbound = Outbound {
            mailbox: &mailbox,
            theme: &writers.theme,
            appearance: &writers.appearance,
        };
        let (events, receiver) = unbounded();
        let watching = Watching {
            watcher: FakeWatch::default(),
            events: receiver,
        };
        let mut session =
            ConfigLoop::new((&paths(&directory), timing()), &outbound, watching);
        assert_eq!(
            session.watcher.watched,
            vec![directory.path().to_path_buf()]
        );
        session.feed(ConfigMessage::FilesChanged);
        let embedded = cells.theme.take();
        assert_eq!(
            embedded.map(|theme| theme.name.clone()),
            Some("noir".to_string())
        );

        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        std::fs::write(directory.path().join("themes/noir.toml"), HAND_EDITED_THEME)
            .unwrap();
        events.send(Ok(notify::Event::default())).unwrap();
        let (_commands, inbox) = unbounded();
        assert!(matches!(session.wait(&inbox), Wake::FilesystemChange));
        session.feed(ConfigMessage::FilesChanged);

        let edited = cells.theme.take();
        assert!(edited.is_some(), "a changed theme file must be published");
    }

    #[test]
    fn a_rejected_input_is_reported_to_the_mailbox() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = unbounded::<Message>();
        let mailbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = cells();
        let outbound = Outbound {
            mailbox: &mailbox,
            theme: &writers.theme,
            appearance: &writers.appearance,
        };
        let (_events, receiver) = unbounded();
        let watching = Watching {
            watcher: FakeWatch::default(),
            events: receiver,
        };
        let mut unselected = paths(&directory);
        unselected.theme = None;
        let mut session = ConfigLoop::new((&unselected, timing()), &outbound, watching);

        let halt = session.feed(ConfigMessage::Read {
            file: ConfigFile::Theme,
            text: None,
        });

        assert!(matches!(halt, Halt::Continue));
        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver(
                Driver::Config,
                DriverMessage::Rejected { input: "Read" }
            )]
        );
        assert!(matches!(
            session.feed(ConfigMessage::FilesChanged),
            Halt::Continue
        ));
    }

    #[test]
    fn a_closed_mailbox_ends_the_config_driver() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, messages) = unbounded();
        let (writers, _cells, doorbell) = cells();
        let thread = spawn_config(
            ConfigParts {
                paths: paths(&directory),
                timing: timing(),
                theme: writers.theme,
                appearance: writers.appearance,
            },
            &mailbox,
        )
        .unwrap();

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
            Err(SendError(Message::Driver(
                Driver::Config,
                DriverMessage::Stopped
            )))
        );
        let content =
            std::fs::read_to_string(directory.path().join("sifr-ui.toml")).unwrap();
        assert!(
            content.contains("key_hints"),
            "the pending save must land on disk"
        );
    }
}
