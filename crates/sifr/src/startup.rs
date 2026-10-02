use std::path::{Path, PathBuf};

use clap::Parser;
use config::{
    APPEARANCE_FILE_NAME,
    AppearanceFile,
    CONFIG_FILE_NAME,
    ConfigToml,
    Rgb,
    ThemeColors,
    ThemeFile,
    parse_appearance,
    parse_config,
};
use kernel::{
    Bounded,
    domain::{
        Percent,
        Shuffle,
        Startup,
        ThemeChoice,
        ThemeName,
        appearance_rows::appearance_settings,
    },
    playlist::{PlaylistFileName, PlaylistSource},
};
use library::LibraryDirs;
use widgets::{Colors, Theme, ThemeSeed};

use crate::error::Error;

pub(crate) struct Boot {
    pub(crate) startup: Startup,
    pub(crate) paths: runtime::StartupPaths,
    pub(crate) look: Look,
}

#[derive(Debug, Clone)]
pub(crate) struct Look {
    pub(crate) theme: ThemeFile,
}

struct ReadFile<T> {
    value: T,
    text: Option<String>,
    warning: Option<String>,
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

fn user_config_dir() -> PathBuf {
    dirs::config_dir()
        .map_or_else(|| PathBuf::from("."), |directory| directory.join("sifr"))
}

fn read_config_file(path: &Path) -> Result<(ConfigToml, Option<String>), Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_config(&text) {
            Ok(file) => Ok((file, Some(text))),
            Err(source) => Err(Error::ConfigParse {
                path: path.to_path_buf(),
                source,
            }),
        },
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok((ConfigToml::default(), None))
        }
        Err(source) => Err(Error::ConfigRead {
            path: path.to_path_buf(),
            source,
        }),
    }
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
) -> runtime::ConfigPaths {
    runtime::ConfigPaths {
        config: config_file,
        appearance: appearance_path(config_dir),
        themes: themes_dir(config_dir),
        theme: Some(config::resolve_theme(choice).to_string()),
        seen: runtime::SeenTexts::default(),
    }
}

fn fallback<T>(fallback: T, path: &Path, source: &std::io::Error) -> ReadFile<T> {
    ReadFile {
        value: fallback,
        text: None,
        warning: Some(format!("cannot read {}: {source}", path.display())),
    }
}

fn read_appearance(path: &Path) -> ReadFile<AppearanceFile> {
    let text = match library::files::read_if_present(path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return ReadFile {
                value: AppearanceFile::default(),
                text: None,
                warning: None,
            };
        }
        Err(source) => return fallback(AppearanceFile::default(), path, &source),
    };
    match parse_appearance(&text) {
        Ok(value) => ReadFile {
            value,
            text: Some(text),
            warning: None,
        },
        Err(error) => ReadFile {
            value: AppearanceFile::default(),
            text: Some(text),
            warning: Some(error.to_string()),
        },
    }
}

pub(crate) fn theme_from_file(file: ThemeFile) -> Theme {
    let ThemeColors {
        background,
        foreground,
        bright_foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    } = file.colors;
    let palette = ThemeSeed {
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
        name: file.name,
        colors: Colors::derive(&palette),
        scanning_label: file.scanning_label,
    }
}

pub(crate) fn fallback_theme_file() -> ThemeFile {
    ThemeFile {
        name: ThemeName::from_static("fallback"),
        colors: ThemeColors {
            background: Rgb([0, 0, 0]),
            foreground: Rgb([0xff, 0xff, 0xff]),
            bright_foreground: Rgb([0xff, 0xff, 0xff]),
            accent: Rgb([0xff, 0xff, 0xff]),
            green: Rgb([0, 0xff, 0]),
            yellow: Rgb([0xff, 0xff, 0]),
            red: Rgb([0xff, 0, 0]),
            window_background: None,
        },
        scanning_label: "scanning…".to_string(),
    }
}

fn stock_theme() -> ThemeFile {
    config::embedded_theme("noir")
        .and_then(|source| config::parse_theme(source, "noir").ok())
        .unwrap_or_else(fallback_theme_file)
}

fn parsed_theme(name: &str, text: &str, seen: Option<String>) -> ReadFile<ThemeFile> {
    match config::parse_theme(text, name) {
        Ok(value) => ReadFile {
            value,
            text: seen,
            warning: None,
        },
        Err(error) => ReadFile {
            value: stock_theme(),
            text: seen,
            warning: Some(error.to_string()),
        },
    }
}

fn embedded_or_stock_theme(name: &str) -> ReadFile<ThemeFile> {
    config::embedded_theme(name).map_or_else(
        || ReadFile {
            value: stock_theme(),
            text: None,
            warning: Some(format!("no theme named `{name}`")),
        },
        |embedded| parsed_theme(name, embedded, None),
    )
}

fn read_theme(choice: &ThemeChoice, themes: &Path) -> ReadFile<ThemeFile> {
    let resolved = config::resolve_theme(choice);
    let name = resolved.as_str();
    let path = themes.join(config::theme_file_name(name));
    match library::files::read_if_present(&path) {
        Ok(Some(text)) => parsed_theme(name, &text, Some(text.clone())),
        Ok(None) => embedded_or_stock_theme(name),
        Err(source) => fallback(stock_theme(), &path, &source),
    }
}

fn with_look(
    startup: Startup,
    config_dir: &Path,
    paths: runtime::StartupPaths,
) -> Boot {
    let appearance = read_appearance(&appearance_path(config_dir));
    let theme = read_theme(&startup.theme, &themes_dir(config_dir));
    let appearance_settings = appearance_settings(appearance.value.appearance());
    let toasts = [appearance.warning, theme.warning]
        .into_iter()
        .flatten()
        .collect();
    let mut paths = paths;
    paths.config.seen.appearance = appearance.text;
    paths.config.seen.theme = theme.text;
    Boot {
        startup: Startup {
            appearance_settings,
            look: appearance.value.look(),
            toast_texts: toasts,
            ..startup
        },
        paths,
        look: Look { theme: theme.value },
    }
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
    startup: &mut Startup,
    library: &LibraryDirs,
    name: &str,
) -> Result<(), Error> {
    let file_name =
        PlaylistFileName::new(name).map_err(|source| Error::PlaylistName {
            name: name.to_owned(),
            source,
        })?;
    let playlist = library::load_playlist(library, &file_name)?;
    startup.playlist_index = playlist.playing_index();
    startup.playlist_tracks = playlist.tracks;
    startup.playlist_source = PlaylistSource::Named;
    Ok(())
}

fn startup_from_file(
    config_toml: ConfigToml,
    music_dir: PathBuf,
    cli: &Cli,
) -> Startup {
    Startup {
        music_dir,
        shuffle: shuffle_requested(cli.shuffle),
        audio: config_toml.audio.into(),
        theme: cli.theme.as_deref().map_or(config_toml.theme, |raw| {
            raw.parse().unwrap_or(ThemeChoice::Auto)
        }),
        volume: cli.volume.map_or(config_toml.volume, Percent::clamped),
        ..Startup::default()
    }
}

pub(crate) fn boot() -> Result<Boot, Error> {
    let cli = Cli::parse();
    let config_dir = user_config_dir();
    let config_file = config_dir.join(CONFIG_FILE_NAME);
    let (config_toml, config_text) = read_config_file(&config_file)?;
    let music_dir = resolved_music_dir(&config_toml, cli.path.clone())?;
    let library = LibraryDirs::user()?;
    let mut startup = startup_from_file(config_toml, music_dir, &cli);
    if let Some(name) = cli.playlist.as_deref() {
        load_named_playlist(&mut startup, &library, name)?;
    }
    let mut paths = runtime::StartupPaths {
        config: config_paths(&config_dir, &startup.theme, config_file),
        library,
    };
    paths.config.seen.config = config_text;
    Ok(with_look(startup, &config_dir, paths))
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use config::AppearanceFile;
    use kernel::domain::{
        Shuffle,
        Startup,
        ThemeChoice,
        ThemeName,
        appearance_rows::appearance_settings,
    };
    use rstest::rstest;

    use crate::startup::{Boot, CONFIG_FILE_NAME, Cli, shuffle_requested, with_look};

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn choice(name: &str) -> ThemeChoice {
        ThemeChoice::Named(ThemeName::new(name.to_string()).unwrap())
    }

    fn booted(directory: &std::path::Path, theme: &ThemeChoice) -> Boot {
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
        with_look(startup, directory, paths)
    }

    #[rstest]
    #[case::valid(Some(COMPACT), "noir", &[])]
    #[case::broken(Some(BROKEN), "noir", &["sifr-ui.toml"])]
    #[case::missing(None, "noir", &[])]
    #[case::embedded_or_stock_theme(None, "ghost", &["ghost"])]
    fn a_broken_appearance_or_missing_theme_falls_back_and_toasts_why(
        #[case] appearance: Option<&str>,
        #[case] theme: &str,
        #[case] toasts: &[&str],
    ) {
        let directory = tempfile::tempdir().unwrap();
        if let Some(text) = appearance {
            std::fs::write(directory.path().join("sifr-ui.toml"), text).unwrap();
        }

        let booted = booted(directory.path(), &choice(theme));

        let expected = if appearance == Some(COMPACT) {
            config::parse_appearance(COMPACT).unwrap()
        } else {
            AppearanceFile::default()
        };
        assert_eq!(booted.startup.look, expected.look());
        assert_eq!(booted.startup.toast_texts.len(), toasts.len());
        for (warning, fragment) in booted.startup.toast_texts.iter().zip(toasts) {
            assert!(warning.contains(fragment), "{warning} lacks {fragment}");
        }
        assert!(!booted.look.theme.name.as_str().is_empty());
    }

    #[test]
    fn an_auto_theme_starts_as_noir_and_is_watched() {
        let directory = tempfile::tempdir().unwrap();

        let booted = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(booted.look.theme.name.as_str(), "noir");
        assert_eq!(booted.paths.config.theme.as_deref(), Some("noir"));
    }

    #[test]
    fn a_named_theme_is_watched_under_its_name() {
        let directory = tempfile::tempdir().unwrap();

        let booted = booted(directory.path(), &choice("ghost"));

        assert_eq!(booted.paths.config.theme.as_deref(), Some("ghost"));
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
        assert_eq!(seen.theme.as_deref(), Some(theme));
    }

    #[test]
    fn the_custom_settings_and_the_painter_see_the_same_appearance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();

        let booted = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(
            booted.startup.appearance_settings,
            appearance_settings(
                config::parse_appearance(COMPACT).unwrap().appearance()
            )
        );
        assert_ne!(
            booted.startup.appearance_settings,
            appearance_settings(AppearanceFile::default().appearance())
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
