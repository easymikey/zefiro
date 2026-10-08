use std::convert::Infallible;

use config::driver::{ConfigDriver, message::ConfigMessage};
use kernel::{cmd::ConfigCmd, domain::driver::DriverName};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::SpawnSetup,
};

pub(crate) fn spawn_config(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<ConfigCmd>, SpawnError> {
    let paths = setup.paths.config_paths.clone();
    let theme_sender = setup.latest_senders.theme_sender.clone();
    let appearance_sender = setup.latest_senders.appearance_sender.clone();
    let run_job = |job: Infallible| match job {};
    DriverLoop::<ConfigDriver<_, _>, Infallible> {
        row: registry::row(DriverName::Config),
        inbox: setup.inbox.clone(),
        callback_receiver: crossbeam_channel::never(),
        message: Some(ConfigMessage::Started),
        run_job,
    }
    .spawn(move || {
        ConfigDriver::new(
            &paths,
            move |theme| theme_sender.publish(theme),
            move |appearance| appearance_sender.publish(appearance),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use config::driver::paths::ConfigPaths;
    use crossbeam_channel::{Receiver, unbounded};
    use kernel::{
        cmd::{ConfigCmd, ConfigPatch},
        domain::{
            appearance::{
                AppearancePatch,
                AppearancePreset,
                FormatChips,
                KeyHints,
                preset_appearance,
            },
            theme::ThemeName,
        },
        message::{ConfigEvent, Message},
    };

    use crate::{
        driver_thread::DriverThread,
        spawn::{
            config_thread::spawn_config,
            tests::{SETTLE_TIMEOUT, drain, spawned_with, stub_paths},
        },
        spawn_setup::StartupPaths,
    };

    const DISK_TIMEOUT: Duration = Duration::from_secs(3);

    struct ConfigRun {
        thread: DriverThread<ConfigCmd>,
        inbox_receiver: Receiver<Message>,
        doorbell: Receiver<()>,
    }

    impl ConfigRun {
        fn start(dir: &Path) -> Self {
            Self::start_with(&stub_paths(dir))
        }

        fn start_with(paths: &StartupPaths) -> Self {
            let (inbox, inbox_receiver) = unbounded();
            let (thread, _latest_receivers, doorbell) =
                spawned_with(spawn_config, paths, &inbox);
            Self {
                thread,
                inbox_receiver,
                doorbell,
            }
        }

        fn send(&self, cmd: ConfigCmd) {
            self.thread.cmd_sender.send(cmd).unwrap();
        }

        fn stop(self) {
            drop(self.thread.cmd_sender);
            self.thread.handle.join().unwrap();
        }
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
        drain(&run.inbox_receiver);

        std::fs::write(
            directory.path().join("sifr-ui.toml"),
            "[window]\nkey_hints = false\n",
        )
        .unwrap();

        let reloaded =
            std::iter::from_fn(|| run.inbox_receiver.recv_timeout(DISK_TIMEOUT).ok())
                .any(|message| {
                    matches!(
                        message,
                        Message::Config(ConfigEvent::AppearanceReloaded(_))
                    )
                });
        assert!(reloaded, "a hand edit after spawn must reach the shell");
        run.stop();
    }

    #[test]
    fn a_hand_edit_of_the_startup_theme_reaches_the_shell() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        let stub = stub_paths(directory.path());
        let paths = StartupPaths {
            config_paths: ConfigPaths {
                theme_name: Some(ThemeName::from_static("noir")),
                ..stub.config_paths
            },
            ..stub
        };
        let run = ConfigRun::start_with(&paths);
        drain(&run.inbox_receiver);

        std::fs::write(
            directory.path().join("themes/noir.toml"),
            "name = \"noir\"\n[colors]\nbackground = \"#010101\"\nmuted_foreground = \"#020202\"\nforeground = \"#030303\"\naccent = \"#040404\"\ngreen = \"#050505\"\nyellow = \"#060606\"\nred = \"#070707\"\n",
        )
        .unwrap();

        let reloaded =
            std::iter::from_fn(|| run.inbox_receiver.recv_timeout(DISK_TIMEOUT).ok())
                .any(|message| {
                    matches!(message, Message::Config(ConfigEvent::ThemeReloaded(_)))
                });
        assert!(
            reloaded,
            "a hand edit of the watched theme must reach the shell"
        );
        run.stop();
    }

    fn themes_loaded(message: Message) -> Option<Vec<ThemeName>> {
        let Message::Config(ConfigEvent::ThemesLoaded { theme_names, .. }) = message
        else {
            return None;
        };
        Some(theme_names)
    }

    #[test]
    fn a_themes_list_reaches_the_kernel_with_the_embedded_names() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), "").unwrap();
        let run = ConfigRun::start(directory.path());

        let themes = drain(&run.inbox_receiver)
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
        drain(&run.inbox_receiver);
        run.doorbell.try_iter().for_each(drop);

        run.send(ConfigCmd::SetAppearance(AppearancePatch::from(
            preset_appearance(AppearancePreset::Noir),
        )));

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
        drain(&run.inbox_receiver);
        run.doorbell.try_iter().for_each(drop);

        run.send(ConfigCmd::Save(ConfigPatch {
            theme_name: Some(ThemeName::from_static("noir")),
            ..ConfigPatch::default()
        }));
        run.send(ConfigCmd::SetAppearance(AppearancePatch {
            format_chips: Some(FormatChips::Shown),
            ..AppearancePatch::default()
        }));

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
        drain(&run.inbox_receiver);

        run.send(ConfigCmd::SetAppearance(AppearancePatch {
            key_hints: Some(KeyHints::Hidden),
            ..AppearancePatch::default()
        }));
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
