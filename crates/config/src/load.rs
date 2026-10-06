use std::path::Path;

use kernel::domain::{
    config::{ConfigError, ConfigName, Diagnostic},
    theme::ThemeName,
};

use crate::{
    appearance_file::{TomlAppearance, parse_appearance},
    config_file::{TomlSettings, parse_config},
    driver::{
        files::{read_if_present, unreadable},
        paths::{ConfigPaths, SeenTexts},
    },
    embedded_theme::{embedded_theme, resolve_theme},
    error::Error,
    file_name::theme_file_path,
    theme_file::{TomlTheme, parse_theme},
};

#[must_use]
#[derive(Debug, Clone)]
pub struct Loaded {
    pub settings: TomlSettings,
    pub toml_appearance: TomlAppearance,
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

pub fn load(paths: &ConfigPaths) -> Loaded {
    let config = read_parsed(&paths.config, ConfigName::Config, parse_config);
    let appearance =
        read_parsed(&paths.appearance, ConfigName::Appearance, parse_appearance);
    let theme_name = paths
        .theme
        .clone()
        .unwrap_or_else(|| resolve_theme(&config.value.theme));
    let (theme, theme_error) = match read_theme(&theme_name, &paths.themes) {
        Ok(theme) => (Some(theme), None),
        Err(error) => (None, Some(error)),
    };
    Loaded {
        texts: SeenTexts {
            appearance: appearance.text,
            config: config.text,
        },
        errors: [config.error, appearance.error, theme_error]
            .into_iter()
            .flatten()
            .collect(),
        settings: config.value,
        toml_appearance: appearance.value,
        theme_name,
        toml_theme: theme,
    }
}

pub(crate) fn theme_parsed(
    name: &ThemeName,
    text: Option<&str>,
) -> Result<TomlTheme, ConfigError> {
    let source = text
        .or_else(|| embedded_theme(name.as_str()))
        .ok_or_else(|| {
            ConfigError::from(Diagnostic::from_error(&Error::UnknownTheme(
                name.clone(),
            )))
        })?;
    parse_theme(source, name.as_str())
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
        Err(source) => {
            return Parsed {
                value: T::default(),
                text: None,
                error: Some((name.clone(), unreadable(name, &source))),
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
        read_if_present(&theme_file_path(themes_dir, name)).map_err(|source| {
            (
                config_name.clone(),
                unreadable(config_name.clone(), &source),
            )
        })?;
    theme_parsed(name, text.as_deref()).map_err(|error| (config_name, error))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::domain::{
        config::{ConfigError, ConfigName},
        io_error::IoError,
        keymap::{Action, KeyOverride, KeymapOverrides},
        theme::{ThemeChoice, ThemeName, ThemeNameError},
    };
    use rstest::rstest;

    use crate::{
        appearance_file::TomlAppearance,
        config_file::TomlSettings,
        driver::paths::{ConfigPaths, SeenTexts},
        embedded_theme::{STOCK_THEME, embedded_theme},
        load::{Loaded, load},
    };

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn paths(directory_path: &Path, theme: Option<&'static str>) -> ConfigPaths {
        ConfigPaths {
            config: directory_path.join("config.toml"),
            appearance: directory_path.join("sifr-ui.toml"),
            themes: directory_path.join("themes"),
            default_music_dir: None,
            theme: theme.map(ThemeName::from_static),
            seen: SeenTexts::default(),
        }
    }

    fn loaded(directory_path: &Path, theme: Option<&'static str>) -> Loaded {
        load(&paths(directory_path, theme))
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
    fn nothing_on_disk_loads_defaults_and_the_embedded_theme() {
        let directory = tempfile::tempdir().unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.settings, TomlSettings::default());
        assert_eq!(loaded.toml_appearance, TomlAppearance::default());
        assert_eq!(loaded.theme_name.as_str(), STOCK_THEME);
        assert_eq!(
            loaded.toml_theme.map(|theme| theme.name),
            Some(loaded.theme_name)
        );
        assert_eq!(loaded.texts, SeenTexts::default());
        assert!(loaded.errors.is_empty());
    }

    #[test]
    fn the_theme_comes_from_the_config_unless_the_paths_name_one() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "theme = \"ghost\"\n")
            .unwrap();

        let from_config = loaded(directory.path(), None);
        let from_paths = loaded(directory.path(), Some("noir"));

        assert_eq!(
            from_config.settings.theme,
            ThemeChoice::Named(ThemeName::from_static("ghost"))
        );
        assert_eq!(from_config.theme_name.as_str(), "ghost");
        assert_eq!(from_paths.theme_name.as_str(), "noir");
        assert!(from_paths.toml_theme.is_some());
    }

    #[test]
    fn a_keymap_in_the_config_is_loaded_and_its_text_is_seen() {
        let directory = tempfile::tempdir().unwrap();
        let text = "[keymap]\nquit = \"q\"\n";
        std::fs::write(directory.path().join("config.toml"), text).unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(
            loaded.settings.keymap(),
            KeymapOverrides::from([(Action::Quit, KeyOverride::from("q"))])
        );
        assert_eq!(loaded.texts.config.as_deref(), Some(text));
    }

    #[test]
    fn a_broken_config_falls_back_to_defaults_and_reports_why() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "volume = \"loud\"\n")
            .unwrap();

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.settings, TomlSettings::default());
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
            std::fs::write(directory.path().join("sifr-ui.toml"), text).unwrap();
        }

        let loaded = loaded(directory.path(), None);

        assert_eq!(loaded.toml_appearance == TomlAppearance::default(), !parsed);
        assert_eq!(loaded.texts.appearance.as_deref(), text);
        assert_eq!(loaded.errors.len(), errors);
    }

    #[test]
    fn a_valid_user_theme_is_read() {
        let text = embedded_theme("noir").unwrap();
        let directory = mine_with(Some(text));

        let loaded = loaded(directory.path(), Some("mine"));

        assert_eq!(
            loaded.toml_theme,
            Some(crate::theme_file::parse_theme(text, "mine").unwrap())
        );
        assert!(loaded.errors.is_empty());
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
                    source: IoError::from(std::io::ErrorKind::IsADirectory),
                }
            )]
        );
    }

    #[rstest]
    #[case::parent("../x")]
    #[case::nested("a/b")]
    fn a_theme_name_with_a_path_in_it_cannot_be_built(#[case] name: &str) {
        assert_eq!(ThemeName::new(name.to_owned()), Err(ThemeNameError::Path));
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
