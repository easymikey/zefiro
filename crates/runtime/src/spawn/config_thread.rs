use std::{convert::Infallible, path::Path};

use config::{ConfigDriver, ConfigEffect, ConfigMessage, ConfigPaths};
use kernel::{
    ConfigCmd,
    IoError,
    domain::{ConfigError, DriverName, ThemeChoice},
};

use crate::{
    driver::{DriverLoop, DriverThread, LoopEffect},
    error::Error,
    jobs::Jobs,
    registry,
    spawn::SpawnSetup,
};

fn config_split(
    effect: ConfigEffect,
) -> LoopEffect<ConfigEffect, Infallible, ConfigMessage> {
    match effect {
        ConfigEffect::Watch(path) => LoopEffect::Watch {
            path,
            item: config_watched,
        },
        ConfigEffect::After { delay, revision } => LoopEffect::After {
            delay,
            message: ConfigMessage::Elapsed(revision),
        },
        effect @ (ConfigEffect::Read { .. }
        | ConfigEffect::List(_)
        | ConfigEffect::SaveConfig(_)
        | ConfigEffect::SaveAppearance(_)
        | ConfigEffect::Publish(_)) => LoopEffect::Execute(effect),
    }
}

fn config_watched(_path: &Path, changed: Result<(), IoError>) -> ConfigMessage {
    match changed {
        Ok(()) => ConfigMessage::FilesChanged,
        Err(kind) => ConfigMessage::Error(ConfigError::Watch(kind)),
    }
}

pub(crate) fn spawn_config(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<ConfigCmd>, Error> {
    let paths = ConfigPaths {
        theme: match setup.theme {
            ThemeChoice::Named(name) => Some(name.clone()),
            ThemeChoice::Auto => None,
        },
        ..setup.paths.config.clone()
    };
    let writer = setup.writers.theme.clone();
    let jobs = Jobs {
        split: config_split,
        run: |job: Infallible| match job {},
    };
    DriverLoop::<ConfigDriver<_>, Infallible> {
        row: registry::row(DriverName::Config),
        inbox: setup.inbox.clone(),
        heard: crossbeam_channel::never(),
        seed: Some(ConfigMessage::Started),
        jobs,
    }
    .spawn(move || ConfigDriver::new(&paths, move |theme| writer.publish(theme)))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use crossbeam_channel::{Receiver, unbounded};
    use kernel::{
        ConfigCmd,
        ConfigEvent,
        ConfigPatch,
        Message,
        domain::{OptionIndex, Startup, ThemeName, appearance_rows::AppearanceField},
    };

    use crate::{
        driver::DriverThread,
        spawn::{
            SpawnSetup,
            config_thread::spawn_config,
            tests::{RECV_TIMEOUT, stub_paths},
        },
    };

    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);
    const DISK_TIMEOUT: Duration = Duration::from_secs(3);

    struct ConfigRun {
        thread: DriverThread<ConfigCmd>,
        messages: Receiver<Message>,
        doorbell: Receiver<()>,
    }

    impl ConfigRun {
        fn start(directory: &Path) -> Self {
            let paths = stub_paths(directory);
            let (inbox, messages) = unbounded();
            let (model, _cmd) = kernel::startup(Startup::default());
            let (writers, _cells, doorbell) = crate::latest::latest_channels();
            let thread = spawn_config(&SpawnSetup {
                audio: &model.settings.audio,
                theme: &model.themes.selected,
                paths: &paths,
                inbox: &inbox,
                writers: &writers,
                #[cfg(target_os = "macos")]
                macos: &crate::macos::MacosChannel::new(),
            })
            .unwrap();
            Self {
                thread,
                messages,
                doorbell,
            }
        }

        fn send(&self, cmd: ConfigCmd) {
            self.thread.commands.send(cmd).unwrap();
        }

        fn stop(self) {
            drop(self.thread.commands);
            self.thread.handle.join().unwrap().unwrap();
        }
    }

    fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        collected.extend(std::iter::from_fn(|| {
            receiver.recv_timeout(SETTLE_TIMEOUT).ok()
        }));
        collected
    }

    fn option_at(field: AppearanceField, position: usize) -> OptionIndex {
        kernel::domain::appearance_rows::appearance_row(field)
            .unwrap()
            .control
            .count()
            .index(position)
            .unwrap()
    }

    fn wait_for_content(path: &Path, marker: &str) -> Option<String> {
        let deadline = std::time::Instant::now() + DISK_TIMEOUT;
        while std::time::Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(path)
                && text.contains(marker)
            {
                return Some(text);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn a_hand_edit_after_spawn_reaches_the_shell() {
        let directory = tempfile::tempdir().unwrap();
        let run = ConfigRun::start(directory.path());
        run.doorbell.try_iter().for_each(drop);
        drain(&run.messages);

        std::fs::write(
            directory.path().join("sifr-ui.toml"),
            "[window]\nkey_hints = false\n",
        )
        .unwrap();

        let reloaded =
            std::iter::from_fn(|| run.messages.recv_timeout(DISK_TIMEOUT).ok()).any(
                |message| {
                    matches!(
                        message,
                        Message::Config(ConfigEvent::AppearanceReloaded(_))
                    )
                },
            );
        assert!(reloaded, "a hand edit after spawn must reach the shell");
        run.stop();
    }

    fn themes_loaded(message: Message) -> Option<Vec<ThemeName>> {
        let Message::Config(ConfigEvent::ThemesLoaded(themes)) = message else {
            return None;
        };
        Some(themes)
    }

    #[test]
    fn a_themes_list_reaches_the_kernel_with_the_embedded_names() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), "").unwrap();
        let run = ConfigRun::start(directory.path());

        let themes = drain(&run.messages)
            .into_iter()
            .find_map(themes_loaded)
            .unwrap();

        assert!(themes.contains(&ThemeName::from_static("mine")));
        assert!(themes.contains(&ThemeName::from_static("noir")));
        run.stop();
    }

    #[test]
    fn a_preset_write_lands_on_disk_and_never_selects_a_theme() {
        let directory = tempfile::tempdir().unwrap();
        let run = ConfigRun::start(directory.path());
        drain(&run.messages);
        run.doorbell.try_iter().for_each(drop);

        run.send(ConfigCmd::Setting {
            field: AppearanceField::Preset,
            option: option_at(AppearanceField::Preset, 1),
        });

        let text = wait_for_content(&directory.path().join("sifr-ui.toml"), "milkdrop")
            .unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            parsed
                .get("cover")
                .and_then(|cover| cover.get("mode"))
                .and_then(toml::Value::as_str),
            Some("milkdrop")
        );
        assert!(
            run.doorbell.try_recv().is_err(),
            "an appearance write alone must never select a theme"
        );
        run.stop();
    }

    #[test]
    fn writers_never_create_files_in_the_repo_root() {
        let directory = tempfile::tempdir().unwrap();
        let run = ConfigRun::start(directory.path());
        drain(&run.messages);
        run.doorbell.try_iter().for_each(drop);

        run.send(ConfigCmd::Save(
            ConfigPatch::builder()
                .theme(ThemeName::from_static("noir"))
                .build(),
        ));
        run.send(ConfigCmd::Setting {
            field: AppearanceField::FormatChips,
            option: option_at(AppearanceField::FormatChips, 1),
        });

        let config_path = directory.path().join("config.toml");
        let appearance_path = directory.path().join("sifr-ui.toml");
        assert!(wait_for_content(&config_path, "theme").is_some());
        assert!(wait_for_content(&appearance_path, "format_chips").is_some());
        assert!(
            run.doorbell.recv_timeout(SETTLE_TIMEOUT).is_err(),
            "a write we made ourselves must never come back as a reload"
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(!root.join("sifr-ui.toml").exists());
        assert!(!root.join("config.toml").exists());
        run.stop();
    }

    #[test]
    fn flush_then_a_closed_inbox_leaves_the_pending_save_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let run = ConfigRun::start(directory.path());
        drain(&run.messages);

        run.send(ConfigCmd::Setting {
            field: AppearanceField::KeyHints,
            option: option_at(AppearanceField::KeyHints, 1),
        });
        run.send(ConfigCmd::Flush);
        run.stop();

        let content =
            std::fs::read_to_string(directory.path().join("sifr-ui.toml")).unwrap();
        assert!(
            content.contains("key_hints"),
            "the pending save must land on disk"
        );
    }
}
