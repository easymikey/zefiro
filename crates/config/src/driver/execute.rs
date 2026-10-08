use std::{io, path::Path};

use kernel::{
    domain::{
        config::{ConfigError, ConfigName, Diagnostic},
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
        files::{read_error, read_if_present, store},
        message::ConfigMessage,
        watch::{ConfigWatchEffect, ConfigWatchMessage, SavedFile},
    },
    patch::{patched_appearance_text, patched_config_text},
    theme_file::TomlTheme,
};

const THEME_EXTENSION: &str = "toml";

impl<P: Fn(TomlTheme), A: Fn(Appearance)> Driver for ConfigDriver<P, A> {
    type Effect = ConfigEffect;

    fn execute(&mut self, effect: ConfigEffect) -> Option<ConfigMessage> {
        match effect {
            ConfigEffect::Watch(ConfigWatchEffect::Read { name, path }) => {
                Some(read(name, &path))
            }
            ConfigEffect::Watch(ConfigWatchEffect::List(dir)) => Some(list(&dir)),
            ConfigEffect::SaveConfig(patch) => Some(save(
                SavedFile::Config,
                self.watch.path(SavedFile::Config),
                |text| patched_config_text(text, patch),
            )),
            ConfigEffect::SaveAppearance(patch) => Some(save(
                SavedFile::Appearance,
                self.watch.path(SavedFile::Appearance),
                |text| patched_appearance_text(text, patch),
            )),
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

fn read(config_name: ConfigName, path: &Path) -> ConfigMessage {
    match read_if_present(path) {
        Ok(text) => ConfigMessage::Watch(ConfigWatchMessage::ReadDone {
            name: config_name,
            text,
        }),
        Err(error) => ConfigMessage::Error(read_error(config_name, &error)),
    }
}

fn list(dir: &Path) -> ConfigMessage {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries
            .try_fold(
                (Vec::new(), Vec::new()),
                |(mut theme_names, mut refused), entry| {
                    let path = entry?.path();
                    if path.extension().and_then(|extension| extension.to_str())
                        == Some(THEME_EXTENSION)
                    {
                        let stem = || {
                            path.file_stem().map_or_else(String::new, |stem| {
                                stem.to_string_lossy().into_owned()
                            })
                        };
                        match ThemeName::new(stem()) {
                            Ok(name) => theme_names.push(name),
                            Err(_) => refused.push(stem()),
                        }
                    }
                    Ok::<_, io::Error>((theme_names, refused))
                },
            )
            .map_or_else(
                |error| list_failed(&error),
                |(theme_names, refused)| {
                    ConfigMessage::Watch(ConfigWatchMessage::Listed {
                        theme_names,
                        refused,
                    })
                },
            ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ConfigMessage::Watch(ConfigWatchMessage::Listed {
                theme_names: Vec::new(),
                refused: Vec::new(),
            })
        }
        Err(error) => list_failed(&error),
    }
}

fn list_failed(error: &io::Error) -> ConfigMessage {
    ConfigMessage::Error(ConfigError::ListThemes(error.kind().into()))
}

fn save(
    saved_file: SavedFile,
    path: &Path,
    produce: impl FnOnce(&str) -> Result<String, crate::error::Error>,
) -> ConfigMessage {
    let old_text = match read_if_present(path) {
        Ok(old_text) => old_text,
        Err(error) => return save_failed(saved_file, IoError::from(error.kind())),
    };
    let text = match produce(old_text.as_deref().unwrap_or("")) {
        Ok(text) => text,
        Err(error) => {
            return ConfigMessage::Error(Diagnostic::from_error(&error).into());
        }
    };
    match store(path, text.as_bytes()) {
        Ok(()) => ConfigMessage::Watch(ConfigWatchMessage::Saved { saved_file, text }),
        Err(error) => save_failed(saved_file, error),
    }
}

fn save_failed(saved_file: SavedFile, error: IoError) -> ConfigMessage {
    ConfigMessage::Error(ConfigError::Save {
        name: saved_file.into(),
        error,
    })
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
        update::machine::Driver as _,
    };
    use rstest::{fixture, rstest};

    use crate::{
        driver::{
            Appearance,
            ConfigDriver,
            effect::ConfigEffect,
            message::ConfigMessage,
            paths::{ConfigPaths, SeenTexts},
            watch::{ConfigWatchEffect, ConfigWatchMessage, SavedFile},
        },
        theme_file::TomlTheme,
    };

    type Driver = ConfigDriver<fn(TomlTheme), fn(Appearance)>;

    struct Disk {
        directory: tempfile::TempDir,
        paths: ConfigPaths,
        driver: Driver,
    }

    #[fixture]
    fn disk() -> Disk {
        let directory = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            config_path: directory.path().join("config.toml"),
            appearance_path: directory.path().join("sifr-ui.toml"),
            themes_dir: directory.path().join("themes"),
            default_music_dir: None,
            theme_name: None,
            seen_texts: SeenTexts::default(),
        };
        let driver: Driver = ConfigDriver::new(&paths, drop, drop);
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
        let path = disk.paths.themes_dir.join("noir.toml");

        let message =
            disk.driver
                .execute(ConfigEffect::Watch(ConfigWatchEffect::Read {
                    name: noir(),
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
            .execute(ConfigEffect::Watch(ConfigWatchEffect::List(themes)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed {
                theme_names: vec![ThemeName::from_static("noir")],
                refused: Vec::new()
            }))
        );
    }

    #[rstest]
    fn a_theme_list_reports_a_file_with_a_refused_name(mut disk: Disk) {
        let themes = disk.directory.path().to_path_buf();
        std::fs::write(themes.join("noir.toml"), "").unwrap();
        std::fs::write(themes.join("solar..dark.toml"), "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(ConfigWatchEffect::List(themes)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed {
                theme_names: vec![ThemeName::from_static("noir")],
                refused: vec!["solar..dark".to_string()]
            }))
        );
    }

    #[rstest]
    fn a_missing_directory_lists_as_empty(mut disk: Disk) {
        let missing = disk.paths.themes_dir.clone();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(ConfigWatchEffect::List(missing)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed {
                theme_names: Vec::new(),
                refused: Vec::new()
            }))
        );
    }

    #[rstest]
    fn a_directory_that_is_actually_a_file_is_unreadable(mut disk: Disk) {
        let not_a_directory = disk.paths.themes_dir.clone();
        std::fs::write(&not_a_directory, "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(ConfigWatchEffect::List(
                not_a_directory,
            )));

        assert!(matches!(
            listed,
            Some(ConfigMessage::Error(ConfigError::ListThemes(_)))
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
        ConfigEffect::SaveAppearance(AppearancePatch {
            cover_brackets: Some(CoverBrackets::Shown),
            ..AppearancePatch::default()
        })
    }

    fn save_theme() -> ConfigEffect {
        ConfigEffect::SaveConfig(ConfigPatch {
            theme_name: Some(ThemeName::from_static("dark")),
            ..ConfigPatch::default()
        })
    }

    fn save_crossfade() -> ConfigEffect {
        ConfigEffect::SaveConfig(ConfigPatch {
            crossfade: Some(crossfade(5)),
            ..ConfigPatch::default()
        })
    }

    struct SaveRow {
        name: &'static str,
        text: &'static str,
        save: fn() -> ConfigEffect,
        saved_file: SavedFile,
    }

    #[rstest]
    #[case::appearance_creates_a_minimal_file(SaveRow {
        name: "appearance_missing",
        text: "",
        save: save_cover_brackets,
        saved_file: SavedFile::Appearance,
    })]
    #[case::appearance_updates_one_key_of_an_existing_file(SaveRow {
        name: "appearance_existing",
        text: EXISTING_UI,
        save: save_cover_brackets,
        saved_file: SavedFile::Appearance,
    })]
    #[case::config_creates_a_minimal_file(SaveRow {
        name: "config_missing",
        text: "",
        save: save_theme,
        saved_file: SavedFile::Config,
    })]
    #[case::config_updates_one_key_of_an_existing_file(SaveRow {
        name: "config_existing",
        text: EXISTING_CONFIG,
        save: save_crossfade,
        saved_file: SavedFile::Config,
    })]
    fn a_save_lands_on_disk(#[case] save_row: SaveRow, mut disk: Disk) {
        let path = match save_row.saved_file {
            SavedFile::Appearance => disk.paths.appearance_path.clone(),
            SavedFile::Config => disk.paths.config_path.clone(),
        };
        if !save_row.text.is_empty() {
            std::fs::write(&path, save_row.text).unwrap();
        }

        let saved = disk.driver.execute((save_row.save)());

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Saved {
                saved_file: save_row.saved_file,
                text: text.clone()
            })),
            "the reported text is the text on disk"
        );
        assert!(toml::from_str::<toml::Value>(&text).is_ok());
        let entries = std::fs::read_dir(disk.directory.path()).unwrap().count();
        assert_eq!(entries, 1, "an atomic write leaves no tmp file behind");
        insta::with_settings!({ snapshot_suffix => save_row.name }, {
            insta::assert_snapshot!(text);
        });
    }

    #[rstest]
    #[case::not_a_table("audio = 1\n")]
    #[case::malformed("audio = [\n")]
    fn a_refused_patch_reports_invalid_and_leaves_the_file_unchanged(
        #[case] text: &str,
        mut disk: Disk,
    ) {
        std::fs::write(&disk.paths.config_path, text).unwrap();

        let refused = disk.driver.execute(save_crossfade());

        assert!(
            matches!(refused, Some(ConfigMessage::Error(ConfigError::Parse(_)))),
            "{refused:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&disk.paths.config_path).unwrap(),
            text,
            "a failed save must leave the file untouched"
        );
    }

    #[rstest]
    fn a_theme_list_reports_a_reserved_name_as_refused(mut disk: Disk) {
        let themes = disk.paths.themes_dir.clone();
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("auto.toml"), "").unwrap();

        let listed = disk
            .driver
            .execute(ConfigEffect::Watch(ConfigWatchEffect::List(themes)));

        assert_eq!(
            listed,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Listed {
                theme_names: Vec::new(),
                refused: vec!["auto".to_string()]
            }))
        );
    }

    #[test]
    fn a_save_to_a_path_without_a_parent_reports_missing() {
        let paths = ConfigPaths {
            config_path: "config.toml".into(),
            appearance_path: "sifr-ui.toml".into(),
            themes_dir: "themes".into(),
            default_music_dir: None,
            theme_name: None,
            seen_texts: SeenTexts::default(),
        };
        let mut driver: Driver = ConfigDriver::new(&paths, drop, drop);

        let refused = driver.execute(save_crossfade());

        assert_eq!(
            refused,
            Some(ConfigMessage::Error(ConfigError::Save {
                name: ConfigName::Config,
                error: IoError::Missing,
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

    fn option_at(field: AppearanceField, option_index: usize) -> OptionIndex {
        APPEARANCE_ROWS
            .iter()
            .find(|row| row.field == field)
            .unwrap()
            .control
            .count()
            .index(option_index)
            .unwrap()
    }

    fn full_patch() -> AppearancePatch {
        [
            (AppearanceField::CoverMode, 3),
            (AppearanceField::CoverBrackets, 1),
            (AppearanceField::FormatChips, 0),
            (AppearanceField::ProgressTime, 1),
            (AppearanceField::KeyHints, 1),
            (AppearanceField::LayoutMode, 1),
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
        std::fs::write(&disk.paths.appearance_path, COMMENTED_APPEARANCE).unwrap();

        let saved = disk
            .driver
            .execute(ConfigEffect::SaveAppearance(full_patch()));

        let text = std::fs::read_to_string(&disk.paths.appearance_path).unwrap();
        assert_eq!(
            saved,
            Some(ConfigMessage::Watch(ConfigWatchMessage::Saved {
                saved_file: SavedFile::Appearance,
                text: text.clone()
            }))
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
