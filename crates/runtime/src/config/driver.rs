use config::{AppearanceFile, ThemeFile};
use crossbeam_channel::Sender;
use kernel::{ConfigCmd, ConfigEvent, Message, domain::Driver};

use crate::{
    cells::LatestSender,
    config::{
        ConfigPaths,
        ConfigTiming,
        session::{ConfigLoop, Watching},
    },
    driver::{DriverThread, spawn_driver},
    error::Error,
    registry,
    sender::DriverSender,
};

pub(crate) struct ConfigParts {
    pub(crate) paths: ConfigPaths,
    pub(crate) timing: ConfigTiming,
    pub(crate) theme: LatestSender<ThemeFile>,
    pub(crate) appearance: LatestSender<AppearanceFile>,
}

pub(crate) fn spawn(
    parts: ConfigParts,
    mailbox: &Sender<Message>,
) -> Result<DriverThread<ConfigCmd>, Error> {
    let ConfigParts {
        paths,
        timing,
        theme,
        appearance,
    } = parts;
    let thread = spawn_driver(
        registry::row(Driver::Config),
        move |inbox, mailbox| {
            let outbound = Outbound {
                mailbox,
                theme: &theme,
                appearance: &appearance,
            };
            let watching = Watching::recommended(mailbox);
            ConfigLoop::new((&paths, timing), &outbound, watching).run(inbox);
        },
        mailbox,
    )?;
    Ok(thread)
}

pub(crate) struct Outbound<'a> {
    pub(crate) mailbox: &'a DriverSender<ConfigEvent>,
    pub(crate) theme: &'a LatestSender<ThemeFile>,
    pub(crate) appearance: &'a LatestSender<AppearanceFile>,
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    use crossbeam_channel::Receiver;
    use kernel::{
        ConfigCmd,
        ConfigEvent,
        ConfigPatch,
        Message,
        domain::{ConfigFile, CustomSetting, OptionIndex, SettingId, ThemeName},
    };

    use crate::{
        cells::{Receivers, cells},
        config::{
            ConfigPaths,
            ConfigTiming,
            driver::{ConfigParts, spawn as spawn_config},
        },
        driver::DriverThread,
        error::Error,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    fn spawn(
        paths: ConfigPaths,
        timing: ConfigTiming,
        mailbox: &crossbeam_channel::Sender<Message>,
    ) -> Result<(DriverThread<ConfigCmd>, Receivers, Receiver<()>), Error> {
        let (writers, cells, doorbell) = cells();
        let thread = spawn_config(
            ConfigParts {
                paths,
                timing,
                theme: writers.theme,
                appearance: writers.appearance,
            },
            mailbox,
        )?;
        Ok((thread, cells, doorbell))
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

    fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        while let Ok(item) = receiver.recv_timeout(SETTLE_TIMEOUT) {
            collected.push(item);
        }
        collected
    }

    #[test]
    fn a_hand_edit_of_the_theme_reaches_the_shell_parsed() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let (thread, cells, doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();

        drain(&doorbell);
        let theme = cells.theme.take();
        assert_eq!(
            theme.map(|theme| theme.name.clone()),
            Some("noir".to_string())
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn wait_for_appearance_reload(
        doorbell: &Receiver<()>,
        cells: &Receivers,
        deadline: Instant,
    ) -> bool {
        while Instant::now() < deadline {
            if doorbell.recv_timeout(Duration::from_millis(100)).is_ok()
                && cells.appearance.take().is_some()
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn a_hand_edit_after_spawn_reaches_the_shell_without_polling() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, cells, doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();
        drain(&doorbell);
        let _ = cells.appearance.take();
        drain(&messages);

        std::fs::write(
            directory.path().join("sifr-ui.toml"),
            "[window]\nkey_hints = false\n",
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        assert!(
            wait_for_appearance_reload(&doorbell, &cells, deadline),
            "a hand edit made after spawn must reach the shell through a filesystem event"
        );
        let rows = drain(&messages).into_iter().find_map(custom_rows_reloaded);
        assert!(
            rows.is_some(),
            "a hand edit must send the kernel the file's current row positions"
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_broken_keymap_becomes_a_source_failure() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "[keymap\nnot toml")
            .unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, _doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();

        let failed = drain(&messages).into_iter().any(|message| {
            matches!(
                message,
                Message::Config(ConfigEvent::SourceFailed {
                    source: ConfigFile::Config,
                    ..
                })
            )
        });
        assert!(failed, "a broken keymap must report a source failure");

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_broken_appearance_file_at_start_becomes_a_source_failure_naming_the_file() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("sifr-ui.toml"),
            "[volume]\nmode = \"text\"\n",
        )
        .unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, _doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();

        let failure = drain(&messages).into_iter().find_map(|message| {
            let Message::Config(ConfigEvent::SourceFailed {
                source: ConfigFile::Appearance,
                text,
            }) = message
            else {
                return None;
            };
            Some(text)
        });
        let text =
            failure.expect("a broken appearance file must report a source failure");
        assert!(text.contains("sifr-ui.toml"), "{text:?}");

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_later_broken_appearance_edit_keeps_the_last_good_rows() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();
        drain(&doorbell);
        let first_rows = drain(&messages).into_iter().find_map(custom_rows_reloaded);
        assert!(
            first_rows.is_some(),
            "the stock appearance file must seed the rows once"
        );

        std::fs::write(
            directory.path().join("sifr-ui.toml"),
            "[volume]\nmode = \"text\"\n",
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut later_messages = Vec::new();
        while Instant::now() < deadline {
            if let Ok(message) = messages.recv_timeout(Duration::from_millis(100)) {
                later_messages.push(message);
            }
        }
        let failed = later_messages.iter().any(|message| {
            matches!(
                message,
                Message::Config(ConfigEvent::SourceFailed {
                    source: ConfigFile::Appearance,
                    ..
                })
            )
        });
        assert!(failed, "the broken edit must be reported");
        assert!(
            later_messages
                .into_iter()
                .find_map(custom_rows_reloaded)
                .is_none(),
            "a broken edit must never clear the last good rows"
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn toast_text(message: Message) -> Option<String> {
        let Message::Config(ConfigEvent::Error(failure)) = message else {
            return None;
        };
        Some(failure.to_string())
    }

    fn themes_listed(message: Message) -> Option<Vec<ThemeName>> {
        let Message::Config(ConfigEvent::ThemesLoaded(themes)) = message else {
            return None;
        };
        Some(themes)
    }

    fn custom_rows_reloaded(message: Message) -> Option<Vec<CustomSetting>> {
        let Message::Config(ConfigEvent::CustomRowsReloaded(rows)) = message else {
            return None;
        };
        Some(rows)
    }

    #[test]
    fn an_unreadable_config_shows_a_toast() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, _doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();

        let toast = drain(&messages).into_iter().find_map(toast_text);
        assert!(toast.is_none(), "a missing file must not be unreadable");

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_themes_list_reaches_the_kernel_with_the_embedded_names() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("themes")).unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), "").unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, _doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();

        let themes = drain(&messages)
            .into_iter()
            .find_map(themes_listed)
            .unwrap();
        assert!(themes.contains(&ThemeName::from_static("mine")));
        assert!(themes.contains(&ThemeName::from_static("noir")));

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn wait_for(path: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if path.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn wait_for_content(path: &Path, marker: &str) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
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
    fn config_and_appearance_writers_never_create_files_in_the_repo_root() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        let appearance_path = directory.path().join("sifr-ui.toml");
        let paths = ConfigPaths {
            config: config_path.clone(),
            appearance: appearance_path.clone(),
            themes: directory.path().join("themes"),
            theme: None,
            seen: crate::config::SeenTexts::default(),
        };
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let (thread, _cells, doorbell) = spawn(paths, timing(), &mailbox).unwrap();
        drain(&doorbell);
        thread
            .commands
            .send(ConfigCmd::Save(
                ConfigPatch::builder()
                    .theme(ThemeName::from_static("noir"))
                    .build(),
            ))
            .unwrap();
        let format_chips_id = setting_id(config::AppearanceField::FormatChips);
        thread
            .commands
            .send(ConfigCmd::Setting {
                id: format_chips_id,
                option: option_at(format_chips_id, 1),
            })
            .unwrap();

        assert!(wait_for(&config_path), "config.toml must land on disk");
        assert!(wait_for(&appearance_path), "sifr-ui.toml must land on disk");
        drain(&doorbell);
        assert!(
            doorbell.recv_timeout(SETTLE_TIMEOUT).is_err(),
            "a write we made ourselves must never come back as a reload"
        );
        let root = repo_root();
        assert!(
            !root.join("sifr-ui.toml").exists(),
            "the appearance writer must never create sifr-ui.toml in the repository root"
        );
        assert!(
            !root.join("config.toml").exists(),
            "the config writer must never create config.toml in the repository root"
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    const COMMENTED_APPEARANCE: &str = r#"# sifr-ui.toml
[cover]
# the noir look
style = "vinyl"
brackets = false

[card]
format_chips = true
speed_chip = "always"
"#;

    fn setting_id(field: config::AppearanceField) -> SettingId {
        config::APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap()
            .custom
            .id
    }

    fn option_at(id: SettingId, position: usize) -> OptionIndex {
        config::appearance_row(id)
            .unwrap()
            .custom
            .control
            .count()
            .index(position)
            .unwrap()
    }

    fn appearance_rows() -> [(SettingId, OptionIndex); 6] {
        [
            (config::AppearanceField::CoverStyle, 3),
            (config::AppearanceField::CoverBrackets, 1),
            (config::AppearanceField::FormatChips, 0),
            (config::AppearanceField::ProgressRemaining, 1),
            (config::AppearanceField::KeyHints, 1),
            (config::AppearanceField::LayoutMode, 2),
        ]
        .map(|(field, position)| {
            let id = setting_id(field);
            (id, option_at(id, position))
        })
    }

    fn assert_appearance_rows_landed(parsed: &toml::Value) {
        assert_eq!(text_at(parsed, "cover", "style"), Some("off"));
        assert_eq!(flag_at(parsed, "cover", "brackets"), Some(true));
        assert_eq!(flag_at(parsed, "card", "format_chips"), Some(false));
        assert_eq!(flag_at(parsed, "progress", "remaining"), Some(true));
        assert_eq!(flag_at(parsed, "window", "key_hints"), Some(false));
        assert_eq!(text_at(parsed, "layout", "mode"), Some("compact"));
        assert_eq!(text_at(parsed, "card", "speed_chip"), Some("always"));
    }

    #[test]
    fn save_appearance_round_trips_a_full_patch_onto_an_existing_commented_file() {
        let directory = tempfile::tempdir().unwrap();
        let appearance_path = directory.path().join("sifr-ui.toml");
        std::fs::write(&appearance_path, COMMENTED_APPEARANCE).unwrap();
        let paths = ConfigPaths {
            config: directory.path().join("config.toml"),
            appearance: appearance_path.clone(),
            themes: directory.path().join("themes"),
            theme: None,
            seen: crate::config::SeenTexts::default(),
        };
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let (thread, _cells, doorbell) = spawn(paths, timing(), &mailbox).unwrap();
        drain(&doorbell);

        for (id, position) in appearance_rows() {
            thread
                .commands
                .send(ConfigCmd::Setting {
                    id,
                    option: position,
                })
                .unwrap();
        }

        let text = wait_for_content(&appearance_path, "style = \"off\"").unwrap();
        insta::assert_snapshot!(text);
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_appearance_rows_landed(&parsed);

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn at<'a>(
        parsed: &'a toml::Value,
        table: &str,
        key: &str,
    ) -> Option<&'a toml::Value> {
        parsed.get(table)?.get(key)
    }

    fn text_at<'a>(parsed: &'a toml::Value, table: &str, key: &str) -> Option<&'a str> {
        at(parsed, table, key)?.as_str()
    }

    fn flag_at(parsed: &toml::Value, table: &str, key: &str) -> Option<bool> {
        at(parsed, table, key)?.as_bool()
    }

    #[test]
    fn a_preset_write_lands_on_disk_and_never_selects_a_theme() {
        let directory = tempfile::tempdir().unwrap();
        let appearance_path = directory.path().join("sifr-ui.toml");
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let (thread, _cells, doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();
        drain(&doorbell);

        let preset_id = setting_id(config::AppearanceField::Preset);
        let noir_option = option_at(preset_id, 1);
        thread
            .commands
            .send(ConfigCmd::Setting {
                id: preset_id,
                option: noir_option,
            })
            .unwrap();

        let text = wait_for_content(&appearance_path, "milkdrop").unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            text_at(&parsed, "cover", "style"),
            Some("milkdrop"),
            "the written file must hold noir's full appearance"
        );

        drain(&doorbell);
        assert!(
            doorbell.try_recv().is_err(),
            "an appearance write alone must never select a theme; only ConfigCmd::SelectTheme, which the kernel sends, may do that"
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_appearance_save_sends_no_custom_rows() {
        let directory = tempfile::tempdir().unwrap();
        let appearance_path = directory.path().join("sifr-ui.toml");
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _cells, doorbell) =
            spawn(paths(&directory), timing(), &mailbox).unwrap();
        drain(&doorbell);
        drain(&messages);

        let format_chips_id = setting_id(config::AppearanceField::FormatChips);
        thread
            .commands
            .send(ConfigCmd::Setting {
                id: format_chips_id,
                option: option_at(format_chips_id, 1),
            })
            .unwrap();

        wait_for_content(&appearance_path, "format_chips = true").unwrap();

        let mut settled = Vec::new();
        while let Ok(message) = messages.recv_timeout(SETTLE_TIMEOUT) {
            settled.push(message);
        }
        assert!(
            settled.into_iter().find_map(custom_rows_reloaded).is_none(),
            "a successful appearance write must send no custom rows echo"
        );

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }
}
