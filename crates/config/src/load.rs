use std::{io, path::Path};

use kernel::{
    cmd::ConfigPatch,
    domain::{
        config::{ConfigError, ConfigName, Diagnostic},
        theme::ThemeName,
    },
};

use crate::{
    config_file::{TomlSettings, parse_config},
    driver::{
        files::{read_error, read_if_present, save, save_failed, store},
        paths::{ConfigPaths, SeenTexts},
    },
    embedded_theme::{embedded_theme, theme_name},
    error::Error,
    file_name::theme_file_path,
    patch::patched_config_text,
    theme_file::{TomlTheme, parse_theme},
};

#[must_use]
#[derive(Debug, Clone)]
pub struct Loaded {
    pub toml_settings: TomlSettings,
    pub theme_name: ThemeName,
    pub toml_theme: Option<TomlTheme>,
    pub texts: SeenTexts,
    pub errors: Vec<(ConfigName, ConfigError)>,
}

struct Parsed<T> {
    value: T,
    text: Option<String>,
    error: Option<(ConfigName, ConfigError)>,
}

pub fn load(paths: &ConfigPaths, patch: Option<ConfigPatch>) -> Loaded {
    let save_error = match std::fs::symlink_metadata(&paths.config_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => store(
            &paths.config_path,
            include_str!("../config.toml").as_bytes(),
        )
        .err(),
        Ok(_) | Err(_) => None,
    }
    .map(save_failed)
    .or_else(|| {
        patch.and_then(|patch| {
            save(&paths.config_path, |text| patched_config_text(text, patch)).err()
        })
    })
    .map(|error| (ConfigName::Config, error));
    let config = read_parsed(&paths.config_path, ConfigName::Config, parse_config);
    let theme_name = paths
        .theme_name
        .clone()
        .unwrap_or_else(|| theme_name(&config.value.theme_choice));
    let (theme, theme_error) = match read_theme(&theme_name, &paths.themes_dir) {
        Ok(theme) => (Some(theme), None),
        Err(error) => (None, Some(error)),
    };
    Loaded {
        texts: SeenTexts {
            config: config.text,
        },
        errors: [save_error, config.error, theme_error]
            .into_iter()
            .flatten()
            .collect(),
        toml_settings: config.value,
        theme_name,
        toml_theme: theme,
    }
}

pub(crate) fn theme_parsed(
    name: &ThemeName,
    text: Option<&str>,
) -> Result<TomlTheme, ConfigError> {
    let text = text
        .or_else(|| embedded_theme(name.as_str()))
        .ok_or_else(|| {
            ConfigError::from(Diagnostic::from_error(&Error::UnknownTheme(
                name.clone(),
            )))
        })?;
    parse_theme(text, name.as_str())
        .map_err(|error| Diagnostic::from_error(&error).into())
}

fn read_parsed<T: Default>(
    path: &Path,
    name: ConfigName,
    parse: fn(&str) -> Result<T, Error>,
) -> Parsed<T> {
    let text = match read_if_present(path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return Parsed {
                value: T::default(),
                text: None,
                error: None,
            };
        }
        Err(error) => {
            return Parsed {
                value: T::default(),
                text: None,
                error: Some((name.clone(), read_error(name, &error))),
            };
        }
    };
    match parse(&text) {
        Ok(value) => Parsed {
            value,
            text: Some(text),
            error: None,
        },
        Err(error) => Parsed {
            value: T::default(),
            text: Some(text),
            error: Some((name, Diagnostic::from_error(&error).into())),
        },
    }
}

fn read_theme(
    name: &ThemeName,
    themes_dir: &Path,
) -> Result<TomlTheme, (ConfigName, ConfigError)> {
    let config_name = ConfigName::Theme(name.clone());
    let text =
        read_if_present(&theme_file_path(themes_dir, name)).map_err(|error| {
            (config_name.clone(), read_error(config_name.clone(), &error))
        })?;
    theme_parsed(name, text.as_deref()).map_err(|error| (config_name, error))
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    use kernel::domain::{
        config::{ConfigError, ConfigName},
        io_error::IoError,
        theme::{ThemeChoice, ThemeName},
    };
    use rstest::rstest;

    use crate::{
        config_file::TomlSettings,
        driver::paths::{ConfigPaths, SeenTexts},
        load::{Loaded, load},
    };

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";
    const COMMENTED_CONFIG: &str = include_str!("../config.toml");

    fn paths(directory_path: &Path, theme: Option<&'static str>) -> ConfigPaths {
        ConfigPaths {
            config_path: directory_path.join("config.toml"),
            themes_dir: directory_path.join("themes"),
            default_music_dir: None,
            theme_name: theme.map(ThemeName::from_static),
            seen_texts: SeenTexts::default(),
        }
    }

    fn loaded(directory_path: &Path, theme: Option<&'static str>) -> Loaded {
        load(&paths(directory_path, theme), None)
    }

    fn mine() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("mine"))
    }

    fn mine_with(text: Option<&str>) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let themes = directory.path().join("themes");
        match text {
            Some(text) => {
                std::fs::create_dir(&themes).unwrap();
                std::fs::write(themes.join("mine.toml"), text).unwrap();
            }
            None => std::fs::create_dir_all(themes.join("mine.toml")).unwrap(),
        }
        directory
    }

    #[test]
    fn the_theme_comes_from_the_config_unless_the_paths_name_one() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "theme = \"ghost\"\n")
            .unwrap();

        let from_config = loaded(directory.path(), None);
        let from_paths = loaded(directory.path(), Some("noir"));

        assert_eq!(
            from_config.toml_settings.theme_choice,
            ThemeChoice::Named(ThemeName::from_static("ghost"))
        );
        assert_eq!(from_config.theme_name.as_str(), "ghost");
        assert_eq!(from_paths.theme_name.as_str(), "noir");
        assert!(from_paths.toml_theme.is_some());
    }

    #[test]
    fn a_broken_config_falls_back_to_defaults_and_reports_why() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "volume = \"loud\"\n")
            .unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.toml_settings, TomlSettings::default());
        assert_eq!(loaded.texts.config.as_deref(), Some("volume = \"loud\"\n"));
        assert!(matches!(
            loaded.errors.as_slice(),
            [(ConfigName::Config, ConfigError::Parse(_))]
        ));
    }

    #[rstest]
    #[case::valid(Some(COMPACT), true, 0)]
    #[case::broken(Some(BROKEN), false, 1)]
    #[case::missing(None, false, 0)]
    fn a_broken_appearance_falls_back_and_reports_why(
        #[case] text: Option<&str>,
        #[case] parsed: bool,
        #[case] errors: usize,
    ) {
        let directory = tempfile::tempdir().unwrap();
        if let Some(text) = text {
            std::fs::write(directory.path().join("config.toml"), text).unwrap();
        }

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.toml_settings == TomlSettings::default(), !parsed);
        assert_eq!(
            loaded.texts.config.as_deref(),
            Some(text.unwrap_or(COMMENTED_CONFIG))
        );
        assert_eq!(loaded.errors.len(), errors);
    }

    #[test]
    fn a_missing_config_is_written_from_the_template_and_loads_the_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let config_dir = directory.path().join("zefiro");

        let loaded = loaded(&config_dir, None);

        assert_eq!(
            std::fs::read_to_string(config_dir.join("config.toml")).unwrap(),
            COMMENTED_CONFIG
        );
        assert_eq!(loaded.toml_settings, TomlSettings::default());
        assert_eq!(loaded.texts.config.as_deref(), Some(COMMENTED_CONFIG));
        assert_eq!(loaded.errors, []);
    }

    #[rstest]
    #[case::valid(COMPACT)]
    #[case::broken(BROKEN)]
    #[case::empty("")]
    fn an_existing_config_stays_byte_identical(#[case] text: &str) {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        std::fs::write(&config_path, text).unwrap();

        let seen = loaded(directory.path(), None).texts.config;

        assert_eq!(seen.as_deref(), Some(text));
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), text);
    }

    #[test]
    fn a_dangling_config_link_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        std::os::unix::fs::symlink("dotfiles.toml", &config_path).unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(
            std::fs::read_link(&config_path).unwrap(),
            Path::new("dotfiles.toml")
        );
        assert!(!directory.path().join("dotfiles.toml").exists());
        assert_eq!(loaded.toml_settings, TomlSettings::default());
    }

    #[test]
    fn a_failed_template_write_reports_it_and_loads_the_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let mut permissions =
            std::fs::metadata(directory.path()).unwrap().permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(directory.path(), permissions).unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.toml_settings, TomlSettings::default());
        assert_eq!(loaded.texts.config, None);
        assert_eq!(
            loaded.errors,
            [(
                ConfigName::Config,
                ConfigError::Save {
                    name: ConfigName::Config,
                    error: IoError::Denied,
                }
            )]
        );
    }

    #[test]
    fn a_broken_user_theme_has_no_theme_and_reports_why() {
        let directory = mine_with(Some("colors = 3\n"));

        let loaded = loaded(directory.path(), Some("mine"));

        assert_eq!(loaded.toml_theme, None);
        assert!(matches!(
            loaded.errors.as_slice(),
            [(name, ConfigError::Parse(_))] if *name == mine()
        ));
    }

    #[test]
    fn an_unreadable_user_theme_has_no_theme_and_reports_why() {
        let directory = mine_with(None);

        let loaded = loaded(directory.path(), Some("mine"));

        assert_eq!(loaded.toml_theme, None);
        assert_eq!(
            loaded.errors,
            [(
                mine(),
                ConfigError::Read {
                    name: mine(),
                    error: IoError::from(std::io::ErrorKind::IsADirectory),
                }
            )]
        );
    }

    #[test]
    fn an_unknown_theme_has_no_theme_and_is_reported_under_its_name() {
        let directory = tempfile::tempdir().unwrap();

        let loaded = loaded(directory.path(), Some("ghost"));

        assert_eq!(loaded.toml_theme, None);
        assert_eq!(loaded.theme_name.as_str(), "ghost");
        assert!(matches!(
            loaded.errors.as_slice(),
            [(ConfigName::Theme(_), ConfigError::Parse(_))]
        ));
    }
}
