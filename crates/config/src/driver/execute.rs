use std::{
    io,
    path::{Path, PathBuf},
};

use kernel::{
    domain::{
        config::{ConfigError, ConfigName},
        io_error::IoError,
        theme::ThemeName,
    },
    update::machine::Driver,
};

use crate::{
    driver::{
        Appearance,
        ConfigDriver,
        effect::ConfigEffect,
        files::{read_if_present, store},
        message::ConfigMessage,
        watch::{ConfigWatchMessage, WatchEffect},
    },
    patch::{patched_appearance_text, patched_config_text},
    theme_file::TomlTheme,
};

const THEME_EXTENSION: &str = "toml";

impl<P: Fn(TomlTheme), A: Fn(Appearance)> Driver for ConfigDriver<P, A> {
    type Effect = ConfigEffect;

    fn execute(&mut self, effect: ConfigEffect) -> Option<ConfigMessage> {
        match effect {
            ConfigEffect::Watch(WatchEffect::Read { file, path }) => {
                Some(read(file, &path))
            }
            ConfigEffect::Watch(WatchEffect::List(dir)) => Some(list(&dir)),
            ConfigEffect::SaveConfig(patch) => {
                Some(save(ConfigName::Config, &self.config, |existing| {
                    patched_config_text(existing, patch)
                }))
            }
            ConfigEffect::SaveAppearance(patch) => {
                Some(save(ConfigName::Appearance, &self.appearance, |existing| {
                    patched_appearance_text(existing, patch)
                }))
            }
            ConfigEffect::PublishTheme(theme) => {
                (self.publish_theme)(theme);
                None
            }
            ConfigEffect::PublishAppearance(appearance) => {
                (self.publish_appearance)(appearance);
                None
            }
        }
    }
}

fn read(file: ConfigName, path: &Path) -> ConfigMessage {
    match read_if_present(path) {
        Ok(text) => {
            ConfigMessage::Watch(ConfigWatchMessage::ReadDone { name: file, text })
        }
        Err(error) => ConfigMessage::Error(ConfigError::Unreadable {
            file,
            kind: error.kind().into(),
        }),
    }
}

fn list(dir: &Path) -> ConfigMessage {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.path()))
            .filter(|path| {
                path.as_ref().map_or(true, |path| {
                    path.extension().and_then(|extension| extension.to_str())
                        == Some(THEME_EXTENSION)
                })
            })
            .map(theme_name)
            .collect::<Result<Vec<_>, _>>()
            .map_or_else(ConfigMessage::Error, listed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => listed(Vec::new()),
        Err(error) => {
            ConfigMessage::Error(ConfigError::ThemesUnreadable(error.kind().into()))
        }
    }
}

fn listed(names: Vec<ThemeName>) -> ConfigMessage {
    ConfigMessage::Watch(ConfigWatchMessage::Listed(names))
}

fn theme_name(path: io::Result<PathBuf>) -> Result<ThemeName, ConfigError> {
    let path = path.map_err(|error| ConfigError::invalid(&error))?;
    let stem = path.file_stem().map_or(
        std::borrow::Cow::Borrowed(""),
        std::ffi::OsStr::to_string_lossy,
    );
    ThemeName::new(stem.into_owned()).map_err(|error| ConfigError::invalid(&error))
}

fn save(
    file: ConfigName,
    path: &Path,
    produce: impl FnOnce(&str) -> Result<String, crate::error::Error>,
) -> ConfigMessage {
    let existing = match read_if_present(path) {
        Ok(existing) => existing,
        Err(error) => return save_failed(file, IoError::from(error.kind())),
    };
    let text = match produce(existing.as_deref().unwrap_or("")) {
        Ok(text) => text,
        Err(error) => return ConfigMessage::Error(ConfigError::invalid(&error)),
    };
    match store(path, text.as_bytes()) {
        Ok(()) => ConfigMessage::Saved { file, text },
        Err(kind) => save_failed(file, kind),
    }
}

fn save_failed(file: ConfigName, kind: IoError) -> ConfigMessage {
    ConfigMessage::Error(ConfigError::Save { file, kind })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        cmd::ConfigPatch,
        domain::{
            appearance::{AppearancePatch, CoverBrackets},
            appearance_rows::{APPEARANCE_ROWS, appearance_patch},
            bounded::Bounded,
            config::{ConfigError, ConfigName},
            crossfade::Crossfade,
            io_error::IoError,
            setting_row::{AppearanceField, OptionIndex},
            theme::ThemeName,
        },
        update::machine::Driver,
    };
    use rstest::{fixture, rstest};

    use crate::{
        driver::{
            Appearance,
            ConfigDriver,
            effect::ConfigEffect,
            message::ConfigMessage,
            paths::{ConfigPaths, SeenTexts},
            watch::{ConfigWatchMessage, WatchEffect},
        },
        theme_file::TomlTheme,
    };

    type Sink = ConfigDriver<fn(TomlTheme), fn(Appearance)>;

    struct Disk {
        directory: tempfile::TempDir,
        paths: ConfigPaths,
        driver: Sink,
    }

    #[fixture]
    fn disk() -> Disk {
        let directory = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            config: directory.path().join("config.toml"),
            appearance: directory.path().join("sifr-ui.toml"),
            themes: directory.path().join("themes"),
            default_music_dir: None,
            theme: None,
            seen: SeenTexts::default(),
        };
        let driver: Sink = ConfigDriver::new(&paths, drop, drop);
        Disk {
            directory,
            paths,
            driver,
        }
    }

    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    #[rstest]
    fn a_missing_file_reads_as_no_text(mut disk: Disk) {
        let path = disk.paths.themes.join("noir.toml");

        let message = disk.driver.execute(ConfigEffect::Watch(WatchEffect::Read {
            file: noir(),
            path,
        }));

        assert_eq!(
            message,
            Some(ConfigMessage::Watch(ConfigWatchMessage::ReadDone {
                name: noir(),
                text: None
            }))
        );
    }

    #[rstest]
    fn listing_names_only_the_toml_files_by_stem(mut disk: Disk) {
        let themes = disk.directory.path().to_path_buf();
        std::fs::write(themes.join("noir.toml"), "").unwrap();
        std::fs::write(themes.join("notes.txt"), "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(WatchEffect::List(themes)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed(vec![
                ThemeName::from_static("noir")
            ])))
        );
    }

    #[rstest]
    fn a_missing_directory_lists_as_empty(mut disk: Disk) {
        let missing = disk.paths.themes.clone();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(WatchEffect::List(missing)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed(Vec::new())))
        );
    }

    #[rstest]
    fn a_directory_that_is_actually_a_file_is_unreadable(mut disk: Disk) {
        let not_a_directory = disk.paths.themes.clone();
        std::fs::write(&not_a_directory, "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(WatchEffect::List(not_a_directory)));

        assert!(matches!(
            listed,
            Some(ConfigMessage::Error(ConfigError::ThemesUnreadable(_)))
        ));
    }

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
    }

    const EXISTING_UI: &str =
        "# keep me\n[cover]\nmode = \"vinyl\"\nbrackets = false\n";
    const EXISTING_CONFIG: &str =
        "# keep me\ntheme = \"auto\"\n\n[audio]\ncrossfade = \"0s\"\n";

    fn save_cover_brackets() -> ConfigEffect {
        ConfigEffect::SaveAppearance(
            AppearancePatch::builder()
                .cover_brackets(CoverBrackets::Shown)
                .build(),
        )
    }

    fn save_theme() -> ConfigEffect {
        ConfigEffect::SaveConfig(
            ConfigPatch::builder()
                .theme(ThemeName::from_static("dark"))
                .build(),
        )
    }

    fn save_crossfade() -> ConfigEffect {
        ConfigEffect::SaveConfig(ConfigPatch::builder().crossfade(crossfade(5)).build())
    }

    struct InstallParts {
        name: &'static str,
        existing: &'static str,
        save: fn() -> ConfigEffect,
        file: ConfigName,
    }

    #[rstest]
    #[case::appearance_creates_a_minimal_file(InstallParts {
        name: "appearance_missing",
        existing: "",
        save: save_cover_brackets,
        file: ConfigName::Appearance,
    })]
    #[case::appearance_updates_one_key_of_an_existing_file(InstallParts {
        name: "appearance_existing",
        existing: EXISTING_UI,
        save: save_cover_brackets,
        file: ConfigName::Appearance,
    })]
    #[case::config_creates_a_minimal_file(InstallParts {
        name: "config_missing",
        existing: "",
        save: save_theme,
        file: ConfigName::Config,
    })]
    #[case::config_updates_one_key_of_an_existing_file(InstallParts {
        name: "config_existing",
        existing: EXISTING_CONFIG,
        save: save_crossfade,
        file: ConfigName::Config,
    })]
    fn a_save_lands_on_disk(#[case] landing: InstallParts, mut disk: Disk) {
        let path = match landing.file {
            ConfigName::Appearance => disk.paths.appearance.clone(),
            ConfigName::Config | ConfigName::Theme(_) => disk.paths.config.clone(),
        };
        if !landing.existing.is_empty() {
            std::fs::write(&path, landing.existing).unwrap();
        }

        let saved = disk.driver.execute((landing.save)());

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            Some(ConfigMessage::Saved {
                file: landing.file,
                text: text.clone()
            }),
            "the reported text is the text on disk"
        );
        assert!(toml::from_str::<toml::Value>(&text).is_ok());
        let entries = std::fs::read_dir(disk.directory.path()).unwrap().count();
        assert_eq!(entries, 1, "an atomic write leaves no tmp file behind");
        insta::with_settings!({ snapshot_suffix => landing.name }, {
            insta::assert_snapshot!(text);
        });
    }

    #[rstest]
    #[case::not_a_table("audio = 1\n")]
    #[case::malformed("audio = [\n")]
    fn a_refused_patch_reports_invalid_and_leaves_the_file_unchanged(
        #[case] existing: &str,
        mut disk: Disk,
    ) {
        std::fs::write(&disk.paths.config, existing).unwrap();

        let refused = disk.driver.execute(save_crossfade());

        assert!(
            matches!(refused, Some(ConfigMessage::Error(ConfigError::Invalid(_)))),
            "{refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&disk.paths.config).unwrap(),
            existing,
            "a failed save must leave the file untouched"
        );
    }

    #[rstest]
    fn an_invalid_theme_name_is_reported(mut disk: Disk) {
        let themes = disk.paths.themes.clone();
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("auto.toml"), "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(WatchEffect::List(themes)));

        assert!(
            matches!(listed, Some(ConfigMessage::Error(ConfigError::Invalid(_)))),
            "{listed:?}"
        );
    }

    #[test]
    fn a_save_to_a_path_without_a_parent_reports_missing() {
        let paths = ConfigPaths {
            config: "config.toml".into(),
            appearance: "sifr-ui.toml".into(),
            themes: "themes".into(),
            default_music_dir: None,
            theme: None,
            seen: SeenTexts::default(),
        };
        let mut driver: Sink = ConfigDriver::new(&paths, drop, drop);

        let refused = driver.execute(save_crossfade());

        assert_eq!(
            refused,
            Some(ConfigMessage::Error(ConfigError::Save {
                file: ConfigName::Config,
                kind: IoError::Missing,
            }))
        );
    }

    const COMMENTED_APPEARANCE: &str = r#"# sifr-ui.toml
[cover]
# the noir look
mode = "vinyl"
brackets = false

[card]
format_chips = true
speed_chip = "always"
"#;

    fn option_at(id: AppearanceField, position: usize) -> OptionIndex {
        APPEARANCE_ROWS
            .iter()
            .find(|row| row.field == id)
            .unwrap()
            .control
            .count()
            .index(position)
            .unwrap()
    }

    fn full_patch() -> AppearancePatch {
        [
            (AppearanceField::CoverMode, 3),
            (AppearanceField::CoverBrackets, 1),
            (AppearanceField::FormatChips, 0),
            (AppearanceField::ProgressRemaining, 1),
            (AppearanceField::KeyHints, 1),
            (AppearanceField::LayoutMode, 2),
        ]
        .into_iter()
        .map(|(field, position)| {
            appearance_patch(field, option_at(field, position)).unwrap()
        })
        .reduce(AppearancePatch::then)
        .unwrap()
    }

    fn assert_appearance_rows_landed(parsed: &toml::Value) {
        assert_eq!(text_at(parsed, "cover", "mode"), Some("off"));
        assert_eq!(flag_at(parsed, "cover", "brackets"), Some(true));
        assert_eq!(flag_at(parsed, "card", "format_chips"), Some(false));
        assert_eq!(flag_at(parsed, "progress", "remaining"), Some(true));
        assert_eq!(flag_at(parsed, "window", "key_hints"), Some(false));
        assert_eq!(text_at(parsed, "layout", "mode"), Some("compact"));
        assert_eq!(text_at(parsed, "card", "speed_chip"), Some("always"));
    }

    #[rstest]
    fn save_appearance_round_trips_a_full_patch_onto_an_existing_commented_file(
        mut disk: Disk,
    ) {
        std::fs::write(&disk.paths.appearance, COMMENTED_APPEARANCE).unwrap();

        let saved = disk
            .driver
            .execute(ConfigEffect::SaveAppearance(full_patch()));

        let text = std::fs::read_to_string(&disk.paths.appearance).unwrap();
        assert_eq!(
            saved,
            Some(ConfigMessage::Saved {
                file: ConfigName::Appearance,
                text: text.clone()
            })
        );
        insta::assert_snapshot!(text);
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_appearance_rows_landed(&parsed);
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
}
