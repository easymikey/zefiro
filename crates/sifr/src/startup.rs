use std::path::{Path, PathBuf};

use clap::Parser;
use config::{
    APPEARANCE_FILE_NAME,
    CONFIG_FILE_NAME,
    ConfigToml,
    TomlColors,
    TomlTheme,
    parse_appearance,
    parse_config,
};
use kernel::{
    Bounded,
    IoError,
    domain::{
        ConfigError,
        ConfigName,
        Diagnostic,
        Percent,
        Shuffle,
        Startup,
        ThemeChoice,
        ThemeName,
        appearance::Rgb,
        appearance_rows::appearance_settings,
    },
    playlist::{PlaylistFileName, PlaylistSource},
};
use library::LibraryDirs;
use widgets::{Colors, Theme, ThemeBase};

use crate::error::Error;

const STOCK_THEME: &str = "noir";

const FALLBACK_COLORS: TomlColors = TomlColors {
    background: Rgb([0, 0, 0]),
    foreground: Rgb([0xff, 0xff, 0xff]),
    bright_foreground: Rgb([0xff, 0xff, 0xff]),
    accent: Rgb([0xff, 0xff, 0xff]),
    green: Rgb([0, 0xff, 0]),
    yellow: Rgb([0xff, 0xff, 0]),
    red: Rgb([0xff, 0, 0]),
    window_background: None,
};

pub(crate) struct Launch {
    pub(crate) startup: Startup,
    pub(crate) paths: runtime::StartupPaths,
    pub(crate) theme: TomlTheme,
}

struct Parsed<T> {
    value: T,
    text: Option<String>,
    error: Option<(ConfigName, ConfigError)>,
}

#[derive(Debug, Parser)]
#[command(name = "sifr", about = "A terminal music player")]
struct Cli {
    path: Option<PathBuf>,

    #[arg(long)]
    theme: Option<String>,

    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
    volume: Option<u8>,

    #[arg(long, action = clap::ArgAction::Count)]
    shuffle: u8,

    #[arg(long)]
    playlist: Option<String>,
}

fn shuffle_requested(count: u8) -> Shuffle {
    if count > 0 {
        Shuffle::Enabled
    } else {
        Shuffle::Disabled
    }
}

fn user_config_dir() -> Result<PathBuf, Error> {
    dirs::config_dir()
        .map(|directory| directory.join("sifr"))
        .ok_or(Error::ConfigDirUnset)
}

fn appearance_path(config_dir: &Path) -> PathBuf {
    config_dir.join(APPEARANCE_FILE_NAME)
}

fn themes_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("themes")
}

fn config_paths(
    config_dir: &Path,
    choice: &ThemeChoice,
    config_file: PathBuf,
) -> config::ConfigPaths {
    config::ConfigPaths {
        config: config_file,
        appearance: appearance_path(config_dir),
        themes: themes_dir(config_dir),
        theme: Some(config::resolve_theme(choice)),
        seen: config::SeenTexts::default(),
    }
}

fn fallback<T>(fallback: T, file: ConfigName, source: &std::io::Error) -> Parsed<T> {
    Parsed {
        value: fallback,
        text: None,
        error: Some((
            file.clone(),
            ConfigError::Unreadable {
                file,
                kind: IoError::from(source.kind()),
            },
        )),
    }
}

fn read_parsed<T: Default>(
    path: &Path,
    name: ConfigName,
    parse: fn(&str) -> Result<T, config::Error>,
) -> Parsed<T> {
    let text = match library::files::read_if_present(path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return Parsed {
                value: T::default(),
                text: None,
                error: None,
            };
        }
        Err(source) => return fallback(T::default(), name, &source),
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
            error: invalid(name, &error),
        },
    }
}

pub(crate) fn theme(raw: TomlTheme) -> Theme {
    let TomlColors {
        background,
        foreground,
        bright_foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    } = raw.colors;
    let seed = ThemeBase {
        background,
        foreground,
        bright_foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    };
    Theme {
        name: raw.name,
        colors: Colors::derive(&seed),
        scanning_label: raw.scanning_label,
    }
}

pub(crate) fn fallback_theme() -> TomlTheme {
    TomlTheme {
        name: ThemeName::from_static("fallback"),
        colors: FALLBACK_COLORS,
        scanning_label: "scanning…".to_string(),
    }
}

fn stock_theme() -> Result<TomlTheme, Error> {
    config::embedded_theme(STOCK_THEME).map_or_else(
        || Ok(fallback_theme()),
        |source| config::parse_theme(source, STOCK_THEME).map_err(Error::StockTheme),
    )
}

fn invalid(
    name: ConfigName,
    error: &config::Error,
) -> Option<(ConfigName, ConfigError)> {
    Some((name, ConfigError::Invalid(Diagnostic::from_error(error))))
}

fn parsed_theme(
    name: &ThemeName,
    text: &str,
    seen: Option<String>,
) -> Result<Parsed<TomlTheme>, Error> {
    Ok(match config::parse_theme(text, name.as_str()) {
        Ok(value) => Parsed {
            value,
            text: seen,
            error: None,
        },
        Err(error) => Parsed {
            value: stock_theme()?,
            text: seen,
            error: invalid(ConfigName::Theme(name.clone()), &error),
        },
    })
}

fn embedded_or_stock_theme(name: &ThemeName) -> Result<Parsed<TomlTheme>, Error> {
    match config::embedded_theme(name.as_str()) {
        Some(embedded) => parsed_theme(name, embedded, None),
        None => Ok(Parsed {
            value: stock_theme()?,
            text: None,
            error: invalid(
                ConfigName::Theme(name.clone()),
                &config::Error::UnknownTheme(name.clone()),
            ),
        }),
    }
}

fn read_theme(choice: &ThemeChoice, themes: &Path) -> Result<Parsed<TomlTheme>, Error> {
    let name = config::resolve_theme(choice);
    let path = themes.join(config::theme_file_name(name.as_str()));
    match library::files::read_if_present(&path) {
        Ok(Some(text)) => parsed_theme(&name, &text, Some(text.clone())),
        Ok(None) => embedded_or_stock_theme(&name),
        Err(source) => Ok(fallback(stock_theme()?, ConfigName::Theme(name), &source)),
    }
}

fn startup_with_appearance(
    startup: Startup,
    config_dir: &Path,
    paths: runtime::StartupPaths,
) -> Result<Launch, Error> {
    let appearance = read_parsed(
        &appearance_path(config_dir),
        ConfigName::Appearance,
        parse_appearance,
    );
    let theme = read_theme(&startup.theme, &themes_dir(config_dir))?;
    let appearance_settings = appearance_settings(appearance.value.settings());
    let errors = startup
        .errors
        .into_iter()
        .chain([appearance.error, theme.error].into_iter().flatten())
        .collect();
    let paths = runtime::StartupPaths {
        config: config::ConfigPaths {
            seen: config::SeenTexts {
                appearance: appearance.text,
                ..paths.config.seen
            },
            ..paths.config
        },
        ..paths
    };
    Ok(Launch {
        startup: Startup {
            appearance_settings,
            appearance: appearance.value.appearance(),
            errors,
            ..startup
        },
        paths,
        theme: theme.value,
    })
}

fn resolved_music_dir(
    config_toml: &ConfigToml,
    cli_path: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    let music_dir = cli_path
        .or_else(|| config_toml.music_dir.clone())
        .or_else(dirs::audio_dir)
        .ok_or(Error::MusicDirUnset)?;
    if music_dir.is_dir() {
        Ok(music_dir)
    } else {
        Err(Error::MusicDirMissing { path: music_dir })
    }
}

fn load_named_playlist(
    startup: Startup,
    library: &LibraryDirs,
    name: &str,
) -> Result<Startup, Error> {
    let file_name = PlaylistFileName::new(name).map_err(Error::PlaylistName)?;
    let playlist = library::load_playlist(library, &file_name)?;
    Ok(Startup {
        playlist_index: playlist.playing_index(),
        playlist_tracks: playlist.tracks,
        playlist_source: PlaylistSource::Named,
        ..startup
    })
}

fn merged_startup(
    config_toml: ConfigToml,
    music_dir: PathBuf,
    cli: &Cli,
) -> Result<Startup, Error> {
    let theme = match cli.theme.as_deref() {
        Some(raw) => raw.parse().map_err(Error::ThemeName)?,
        None => config_toml.theme,
    };
    Ok(Startup {
        music_dir,
        shuffle: shuffle_requested(cli.shuffle),
        audio: config_toml.audio.into(),
        theme,
        volume: cli.volume.map_or(config_toml.volume, Percent::clamped),
        ..Startup::default()
    })
}

pub(crate) fn launch() -> Result<Launch, Error> {
    let cli = Cli::parse();
    let config_dir = user_config_dir()?;
    let config_file = config_dir.join(CONFIG_FILE_NAME);
    let Parsed {
        value: config_toml,
        text: config_text,
        error: config_error,
    } = read_parsed(&config_file, ConfigName::Config, parse_config);
    let music_dir = resolved_music_dir(&config_toml, cli.path.clone())?;
    let library = LibraryDirs::user()?;
    let merged = Startup {
        errors: config_error.into_iter().collect(),
        ..merged_startup(config_toml, music_dir, &cli)?
    };
    let startup = match cli.playlist.as_deref() {
        Some(name) => load_named_playlist(merged, &library, name)?,
        None => merged,
    };
    let config = config_paths(&config_dir, &startup.theme, config_file);
    let paths = runtime::StartupPaths {
        config: config::ConfigPaths {
            seen: config::SeenTexts {
                config: config_text,
                ..config.seen
            },
            ..config
        },
        library,
    };
    startup_with_appearance(startup, &config_dir, paths)
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use config::TomlAppearance;
    use kernel::domain::{
        ConfigError,
        ConfigName,
        Shuffle,
        Startup,
        ThemeChoice,
        ThemeName,
        appearance_rows::appearance_settings,
    };
    use rstest::rstest;

    use crate::startup::{
        CONFIG_FILE_NAME,
        Cli,
        Launch,
        read_parsed,
        shuffle_requested,
        startup_with_appearance,
    };

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn choice(name: &str) -> ThemeChoice {
        ThemeChoice::Named(ThemeName::new(name.to_string()).unwrap())
    }

    fn booted(directory: &std::path::Path, theme: &ThemeChoice) -> Launch {
        let paths = runtime::StartupPaths {
            config: crate::startup::config_paths(
                directory,
                theme,
                directory.join(CONFIG_FILE_NAME),
            ),
            library: library::LibraryDirs::user().unwrap(),
        };
        let startup = Startup {
            theme: theme.clone(),
            ..Startup::default()
        };
        startup_with_appearance(startup, directory, paths).unwrap()
    }

    #[rstest]
    fn every_embedded_theme_parses() {
        for &(name, source) in config::EMBEDDED_THEMES {
            assert!(config::parse_theme(source, name).is_ok(), "{name}");
        }
    }

    #[rstest]
    #[case::valid(Some(COMPACT), "noir", vec![])]
    #[case::broken(Some(BROKEN), "noir", vec![ConfigName::Appearance])]
    #[case::missing(None, "noir", vec![])]
    #[case::embedded_or_stock_theme(
        None,
        "ghost",
        vec![ConfigName::Theme(ThemeName::from_static("ghost"))]
    )]
    fn a_broken_appearance_or_missing_theme_falls_back_and_reports_why(
        #[case] appearance: Option<&str>,
        #[case] theme: &str,
        #[case] failed: Vec<ConfigName>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        if let Some(text) = appearance {
            std::fs::write(directory.path().join("sifr-ui.toml"), text).unwrap();
        }

        let booted = booted(directory.path(), &choice(theme));

        let expected = if appearance == Some(COMPACT) {
            config::parse_appearance(COMPACT).unwrap()
        } else {
            TomlAppearance::default()
        };
        assert_eq!(booted.startup.appearance, expected.appearance());
        let names: Vec<_> = booted
            .startup
            .errors
            .iter()
            .map(|(name, error)| {
                assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
                name.clone()
            })
            .collect();
        assert_eq!(names, failed);
        assert!(!booted.theme.name.as_str().is_empty());
    }

    #[test]
    fn a_broken_config_falls_back_to_defaults_and_reports_why() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "volume = \"loud\"\n").unwrap();

        let parsed = read_parsed(&path, ConfigName::Config, config::parse_config);

        assert_eq!(parsed.value, config::ConfigToml::default());
        assert_eq!(parsed.text.as_deref(), Some("volume = \"loud\"\n"));
        assert!(matches!(
            parsed.error,
            Some((ConfigName::Config, ConfigError::Invalid(_)))
        ));
    }

    #[test]
    fn an_auto_theme_starts_as_noir_and_is_watched() {
        let directory = tempfile::tempdir().unwrap();

        let booted = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(booted.theme.name.as_str(), "noir");
        assert_eq!(
            booted.paths.config.theme.as_ref().map(ThemeName::as_str),
            Some("noir")
        );
    }

    #[test]
    fn a_named_theme_is_watched_under_its_name() {
        let directory = tempfile::tempdir().unwrap();

        let booted = booted(directory.path(), &choice("ghost"));

        assert_eq!(
            booted.paths.config.theme.as_ref().map(ThemeName::as_str),
            Some("ghost")
        );
    }

    #[test]
    fn seen_texts_carry_the_boot_texts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();
        std::fs::create_dir(directory.path().join("themes")).unwrap();
        let theme = config::embedded_theme("noir").unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), theme).unwrap();

        let booted = booted(directory.path(), &choice("mine"));

        let seen = booted.paths.config.seen;
        assert_eq!(seen.appearance.as_deref(), Some(COMPACT));
    }

    #[test]
    fn the_custom_settings_and_the_painter_see_the_same_appearance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();

        let booted = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(
            booted.startup.appearance_settings,
            appearance_settings(config::parse_appearance(COMPACT).unwrap().settings())
        );
        assert_ne!(
            booted.startup.appearance_settings,
            appearance_settings(TomlAppearance::default().settings())
        );
    }

    #[test]
    fn volume_within_range_is_accepted() {
        let cli = Cli::try_parse_from(["sifr", "--volume", "80"]).unwrap();

        assert_eq!(cli.volume, Some(80));
    }

    #[test]
    fn volume_above_the_maximum_is_rejected() {
        let error = Cli::try_parse_from(["sifr", "--volume", "150"]).unwrap_err();

        assert!(error.to_string().contains("volume"));
    }

    #[test]
    fn every_flag_parses_into_its_field() {
        let cli = Cli::try_parse_from([
            "sifr",
            "/music",
            "--theme",
            "noir",
            "--volume",
            "42",
            "--shuffle",
            "--playlist",
            "favourites",
        ])
        .unwrap();

        assert_eq!(cli.path.as_deref(), Some(std::path::Path::new("/music")));
        assert_eq!(cli.theme.as_deref(), Some("noir"));
        assert_eq!(cli.volume, Some(42));
        assert_eq!(cli.shuffle, 1);
        assert_eq!(cli.playlist.as_deref(), Some("favourites"));
    }

    #[test]
    fn shuffle_is_off_by_default() {
        let cli = Cli::try_parse_from(["sifr"]).unwrap();

        assert_eq!(cli.shuffle, 0);
    }

    #[rstest]
    #[case::absent(0, Shuffle::Disabled)]
    #[case::present(1, Shuffle::Enabled)]
    #[case::repeated(2, Shuffle::Enabled)]
    fn shuffle_requested_treats_any_count_above_zero_as_enabled(
        #[case] count: u8,
        #[case] expected: Shuffle,
    ) {
        assert_eq!(shuffle_requested(count), expected);
    }
}
