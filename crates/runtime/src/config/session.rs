use std::{path::Path, time::Instant};

use crossbeam_channel::{Receiver, Select, Sender, unbounded};
use kernel::{
    LoadedRequest,
    Message,
    WorkspaceRequest,
    domain::{ConfigFailure, ConfigFile, Toast},
    update::Machine,
};
use notify::RecommendedWatcher;

use crate::{
    config::{
        ConfigPaths,
        ConfigTiming,
        apply::react,
        coalesce::SaveCoalescer,
        disk::{Listing, list_theme_names, read},
        driver::{KeysSighting, Outbound},
        reload::appearance_reload,
        watch::{ConfigChange, ConfigIo, ConfigWatch, ConfigWatchMessage, WatchedFile},
        watcher::{config_directory, register},
    },
    error::SaveError,
    interpret::ConfigCommand,
};

#[derive(Clone, Copy)]
struct SaveTarget {
    file: ConfigFile,
    wrote: fn(String) -> ConfigWatchMessage,
}

enum Wake {
    Command(ConfigCommand),
    FilesystemChange,
    FilesystemEventsLost,
    SaveDeadlineElapsed,
    Stopped,
}

pub(crate) struct ConfigLoop<'a> {
    watch: ConfigWatch,
    watcher: Option<RecommendedWatcher>,
    filesystem_events: Receiver<notify::Result<notify::Event>>,
    coalescer: SaveCoalescer,
    keys_sighting: KeysSighting,
    mailbox: &'a Sender<Message>,
    reloads: &'a Sender<crate::shell::Reload>,
}

impl<'a> ConfigLoop<'a> {
    pub(crate) fn new(
        paths: &ConfigPaths,
        timing: ConfigTiming,
        outbound: &Outbound<'a>,
    ) -> Self {
        let (events, filesystem_events) = unbounded();
        let watcher = notify::recommended_watcher(events).map_or_else(
            |error| {
                let _ = outbound.mailbox.send(watch_failure(&error.to_string()));
                None
            },
            Some,
        );
        let mut loaded = Self {
            watch: ConfigWatch::new(paths),
            watcher,
            filesystem_events,
            coalescer: SaveCoalescer::new(
                paths.config.clone(),
                paths.appearance.clone(),
                timing.save_debounce,
            ),
            keys_sighting: KeysSighting::First,
            mailbox: outbound.mailbox,
            reloads: outbound.reloads,
        };
        loaded.mount(&config_directory(paths));
        loaded
    }

    fn mount(&mut self, directory: &Path) {
        if let Err(error) = register(&mut self.watcher, directory) {
            self.toast(format!("Config watch failed: {error}"));
        }
    }

    pub(crate) fn run(mut self, inbox: &Receiver<ConfigCommand>) {
        self.poll_everything();
        loop {
            match self.wait(inbox) {
                Wake::Command(command) => self.command(command),
                Wake::FilesystemChange => self.poll_everything(),
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                }
                Wake::SaveDeadlineElapsed => {
                    let flushed = self.coalescer.flush_due(Instant::now());
                    self.note_flush(flushed);
                }
                Wake::Stopped => {
                    let flushed = self.coalescer.flush_all();
                    self.note_flush(flushed);
                    return;
                }
            }
        }
    }

    fn wait(&self, inbox: &Receiver<ConfigCommand>) -> Wake {
        let mut select = Select::new();
        let command_index = select.recv(inbox);
        let filesystem_index = select.recv(&self.filesystem_events);
        let selected = match self.coalescer.next_deadline() {
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

    fn command(&mut self, command: ConfigCommand) {
        match command {
            ConfigCommand::Save(patch) => {
                self.coalescer.queue(Instant::now(), patch);
            }
            ConfigCommand::Appearance(patch) => {
                self.coalescer.queue_appearance(Instant::now(), patch);
            }
            ConfigCommand::SelectTheme(name) => {
                self.drive(ConfigWatchMessage::SelectTheme(name));
            }
        }
    }

    fn poll_everything(&mut self) {
        self.drive(ConfigWatchMessage::Poll(WatchedFile::Appearance));
        self.drive(ConfigWatchMessage::Poll(WatchedFile::Keys));
        self.drive(ConfigWatchMessage::Poll(WatchedFile::Theme));
        self.drive(ConfigWatchMessage::PollThemes);
    }

    fn drive(&mut self, message: ConfigWatchMessage) {
        let (watch, io) = step(std::mem::take(&mut self.watch), message);
        self.watch = watch;
        self.act(io);
    }

    fn act(&mut self, io: ConfigIo) {
        match io {
            ConfigIo::Nothing => {}
            ConfigIo::Read { file, path } => self.drive(read(file, &path)),
            ConfigIo::List(dir) => {
                let message = match list_theme_names(&dir) {
                    Listing::Names(names) => ConfigWatchMessage::Listed(names),
                    Listing::Unreadable(detail) => ConfigWatchMessage::Unreadable {
                        file: ConfigFile::ThemeDirectory,
                        detail,
                    },
                };
                self.drive(message);
            }
            ConfigIo::Send(change) => self.apply(change),
        }
    }

    fn apply(&mut self, change: ConfigChange) {
        let outbound = Outbound {
            mailbox: self.mailbox,
            reloads: self.reloads,
        };
        react(change, &outbound, &mut self.keys_sighting);
    }

    fn toast(&self, text: String) {
        self.send(Message::Workspace(WorkspaceRequest::ShowToast(
            Toast::error(text),
        )));
    }

    fn note_flush(&mut self, flushed: crate::config::coalesce::Flushed) {
        if let Some(result) = flushed.config {
            self.note_save(
                result,
                SaveTarget {
                    file: ConfigFile::Keymap,
                    wrote: ConfigWatchMessage::WroteKeys,
                },
            );
        }
        if let Some(result) = flushed.appearance {
            self.note_save(
                result,
                SaveTarget {
                    file: ConfigFile::Appearance,
                    wrote: ConfigWatchMessage::WroteAppearance,
                },
            );
        }
    }

    fn note_save(
        &mut self,
        result: Result<crate::config::write::Written, SaveError>,
        target: SaveTarget,
    ) {
        match result {
            Ok(written) => {
                if target.file == ConfigFile::Appearance {
                    self.reloaded_own_appearance_write(&written.text);
                }
                self.drive((target.wrote)(written.text));
            }
            Err(error) => self.config_failed(ConfigFailure::Save {
                file: target.file,
                detail: error.to_string(),
            }),
        }
    }

    fn reloaded_own_appearance_write(&self, text: &str) {
        if let Ok(file) = appearance_reload(Some(text)) {
            let rows = config::custom_rows(&file);
            self.send(Message::Loaded(LoadedRequest::CustomRowsReloaded(rows)));
        }
    }

    fn config_failed(&self, failure: ConfigFailure) {
        self.send(Message::Workspace(WorkspaceRequest::ConfigFailed(failure)));
    }

    fn send(&self, message: Message) {
        let _ = self.mailbox.send(message);
    }
}

fn step(watch: ConfigWatch, message: ConfigWatchMessage) -> (ConfigWatch, ConfigIo) {
    match watch.transition(message) {
        Ok(pair) => pair,
        Err(rejected) => (rejected.state, ConfigIo::Nothing),
    }
}

fn watch_failure(reason: &str) -> Message {
    Message::Workspace(WorkspaceRequest::ShowToast(Toast::error(format!(
        "Config watch failed: {reason}"
    ))))
}
