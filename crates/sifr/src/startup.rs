use std::path::{Path, PathBuf};

use clap::Parser;
use config::{
    config_file::TomlSettings,
    driver::paths::{ConfigPaths, SeenTexts},
    embedded_theme::{STOCK_THEME, resolve_theme},
    file_name::{APPEARANCE_FILE_NAME, CONFIG_FILE_NAME},
    load::{Loaded, load},
    theme_file::{DEFAULT_SCANNING_LABEL, TomlColors, TomlTheme},
};
use kernel::domain::{
    appearance::Rgb,
    bounded::Bounded,
    percent::Percent,
    playlist::{PlaylistFileName, PlaylistSource},
    startup::{Shuffle, Startup},
    theme::{ThemeChoice, ThemeName},
};
use library::dirs::LibraryDirs;

use crate::error::Error;

const FALLBACK_COLORS: TomlColors = TomlColors {
    background: Rgb([0, 0, 0]),
    muted_foreground: Rgb([0xff, 0xff, 0xff]),
    foreground: Rgb([0xff, 0xff, 0xff]),
    accent: Rgb([0xff, 0xff, 0xff]),
    green: Rgb([0, 0xff, 0]),
    yellow: Rgb([0xff, 0xff, 0]),
    red: Rgb([0xff, 0, 0]),
    window_background: None,
};

pub(crate) struct Launch {
    pub(crate) startup: Startup,
    pub(crate) paths: runtime::spawn_setup::StartupPaths,
    pub(crate) theme: TomlTheme,
    pub(crate) appearance: kernel::domain::appearance::Appearance,
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

fn cli_theme(cli: &Cli) -> Result<Option<ThemeChoice>, Error> {
    cli.theme
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(Error::ThemeName)
}

fn config_paths(config_dir: &Path, choice: Option<&ThemeChoice>) -> ConfigPaths {
    ConfigPaths {
        config: config_dir.join(CONFIG_FILE_NAME),
        appearance: config_dir.join(APPEARANCE_FILE_NAME),
        themes: config_dir.join("themes"),
        default_music_dir: dirs::audio_dir(),
        theme: choice.map(resolve_theme),
        seen: SeenTexts::default(),
    }
}

pub(crate) fn fallback_theme() -> TomlTheme {
    TomlTheme {
        name: ThemeName::from_static("fallback"),
        colors: FALLBACK_COLORS,
        scanning_label: DEFAULT_SCANNING_LABEL.to_owned(),
    }
}

fn stock_theme() -> Result<TomlTheme, Error> {
    config::embedded_theme::embedded_theme(STOCK_THEME).map_or_else(
        || Ok(fallback_theme()),
        |source| {
            config::theme_file::parse_theme(source, STOCK_THEME)
                .map_err(Error::StockTheme)
        },
    )
}

fn resolved_music_dir(
    config_toml: &TomlSettings,
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
    let playlist = library::playlists::load(library, &file_name)?;
    Ok(Startup {
        playlist_index: playlist.playing_index(),
        playlist_tracks: playlist.tracks,
        playlist_source: PlaylistSource::Named,
        ..startup
    })
}

fn merged_startup(config_toml: TomlSettings, music_dir: PathBuf, cli: &Cli) -> Startup {
    Startup {
        music_dir,
        shuffle: shuffle_requested(cli.shuffle),
        keymap: config_toml.keymap(),
        audio: config_toml.audio.into(),
        theme: config_toml.theme,
        volume: cli.volume.map_or(config_toml.volume, Percent::clamped),
        ..Startup::default()
    }
}

fn assembled(
    startup: Startup,
    loaded: Loaded,
    paths: runtime::spawn_setup::StartupPaths,
) -> Result<Launch, Error> {
    let theme = match loaded.toml_theme {
        Some(theme) => theme,
        None => stock_theme()?,
    };
    Ok(Launch {
        startup: Startup {
            appearance: loaded.toml_appearance.settings(),
            errors: loaded.errors,
            ..startup
        },
        paths: runtime::spawn_setup::StartupPaths {
            config: ConfigPaths {
                theme: Some(loaded.theme_name),
                seen: loaded.texts,
                ..paths.config
            },
            ..paths
        },
        theme,
        appearance: loaded.toml_appearance.appearance(),
    })
}

fn start(
    cli: &Cli,
    config_dir: &Path,
    library_dirs: LibraryDirs,
) -> Result<Launch, Error> {
    let choice = cli_theme(cli)?;
    let paths = config_paths(config_dir, choice.as_ref());
    let loaded = load(&paths);
    let music_dir = resolved_music_dir(&loaded.settings, cli.path.clone())?;
    let merged = merged_startup(loaded.settings.clone(), music_dir, cli);
    let themed_startup = Startup {
        theme: choice.unwrap_or(merged.theme),
        ..merged
    };
    let startup = match cli.playlist.as_deref() {
        Some(name) => load_named_playlist(themed_startup, &library_dirs, name)?,
        None => themed_startup,
    };
    assembled(
        startup,
        loaded,
        runtime::spawn_setup::StartupPaths {
            config: paths,
            library: library_dirs,
        },
    )
}

pub(crate) fn launch() -> Result<Launch, Error> {
    start(&Cli::parse(), &user_config_dir()?, LibraryDirs::user()?)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;
    use config::{appearance_file::TomlAppearance, config_file::TomlSettings};
    use kernel::domain::{
        bounded::Bounded,
        config::{ConfigError, ConfigName},
        keymap::{Action, KeyOverride, KeymapOverrides},
        percent::Percent,
        playlist::{PlaylistFileName, PlaylistSource},
        startup::{Shuffle, Startup},
        theme::ThemeName,
    };
    use library::dirs::LibraryDirs;
    use rstest::rstest;

    use crate::{
        error::Error,
        startup::{
            Cli,
            Launch,
            cli_theme,
            load_named_playlist,
            merged_startup,
            resolved_music_dir,
            shuffle_requested,
            start,
        },
    };

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn launched(directory: &Path, theme: Option<&str>) -> Launch {
        let cli = Cli {
            path: Some(directory.to_path_buf()),
            theme: theme.map(str::to_owned),
            volume: None,
            shuffle: 0,
            playlist: None,
        };
        start(&cli, directory, LibraryDirs::under(directory)).unwrap()
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

        let launched = launched(directory.path(), Some(theme));

        let expected = if appearance == Some(COMPACT) {
            config::appearance_file::parse_appearance(COMPACT).unwrap()
        } else {
            TomlAppearance::default()
        };
        assert_eq!(launched.startup.appearance, expected.settings());
        assert_eq!(launched.appearance, expected.appearance());
        let names: Vec<_> = launched
            .startup
            .errors
            .iter()
            .map(|(name, error)| {
                assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
                name.clone()
            })
            .collect();
        assert_eq!(names, failed);
        assert!(!launched.theme.name.as_str().is_empty());
    }

    #[test]
    fn an_auto_theme_starts_as_noir_and_is_watched() {
        let directory = tempfile::tempdir().unwrap();

        let launched = launched(directory.path(), None);

        assert_eq!(launched.theme.name.as_str(), "noir");
        assert_eq!(
            launched.paths.config.theme.as_ref().map(ThemeName::as_str),
            Some("noir")
        );
    }

    #[test]
    fn a_named_theme_is_watched_under_its_name() {
        let directory = tempfile::tempdir().unwrap();

        let launched = launched(directory.path(), Some("ghost"));

        assert_eq!(
            launched.paths.config.theme.as_ref().map(ThemeName::as_str),
            Some("ghost")
        );
    }

    #[test]
    fn seen_texts_carry_the_launch_texts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();
        std::fs::create_dir(directory.path().join("themes")).unwrap();
        let theme = config::embedded_theme::embedded_theme("noir").unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), theme).unwrap();

        let launched = launched(directory.path(), Some("mine"));

        let seen = launched.paths.config.seen;
        assert_eq!(seen.appearance.as_deref(), Some(COMPACT));
    }

    #[test]
    fn the_custom_settings_and_the_painter_see_the_same_appearance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();

        let launched = launched(directory.path(), None);

        assert_eq!(
            launched.startup.appearance,
            config::appearance_file::parse_appearance(COMPACT)
                .unwrap()
                .settings()
        );
        assert_ne!(
            launched.startup.appearance,
            TomlAppearance::default().settings()
        );
    }

    #[test]
    fn a_keymap_in_the_config_is_in_startup_at_launch() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            "[keymap]\nquit = \"q\"\n",
        )
        .unwrap();

        let launched = launched(directory.path(), None);

        assert_eq!(
            launched.startup.keymap,
            KeymapOverrides::from([(Action::Quit, KeyOverride::from("q"))])
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

        assert_eq!(cli.path.as_deref(), Some(Path::new("/music")));
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

    #[test]
    fn the_cli_music_dir_wins_over_the_config() {
        let cli_dir = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let settings = config::config_file::parse_config(&format!(
            "music_dir = {:?}\n",
            config_dir.path()
        ))
        .unwrap();

        let chosen =
            resolved_music_dir(&settings, Some(cli_dir.path().to_path_buf())).unwrap();

        assert_eq!(chosen, cli_dir.path());
        assert_eq!(
            resolved_music_dir(&settings, None).unwrap(),
            config_dir.path()
        );
    }

    #[test]
    fn a_missing_music_dir_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("gone");

        let refused =
            resolved_music_dir(&TomlSettings::default(), Some(missing.clone()));

        assert!(
            matches!(refused, Err(Error::MusicDirMissing { path }) if path == missing)
        );
    }

    #[test]
    fn a_bad_cli_theme_name_is_refused() {
        let cli = Cli::try_parse_from(["sifr", "--theme", ""]).unwrap();

        let refused = cli_theme(&cli);

        assert!(matches!(refused, Err(Error::ThemeName(_))));
    }

    #[rstest]
    #[case::the_cli_volume_wins(&["sifr", "--volume", "80"], 80)]
    #[case::the_config_volume_otherwise(&["sifr"], 10)]
    fn the_volume_comes_from_the_cli_before_the_config(
        #[case] flags: &[&str],
        #[case] expected: u8,
    ) {
        let cli = Cli::try_parse_from(flags).unwrap();
        let settings = config::config_file::parse_config("volume = 10\n").unwrap();

        let merged = merged_startup(settings, PathBuf::from("/music"), &cli);

        assert_eq!(merged.volume, Percent::clamped(expected));
        assert_eq!(merged.music_dir, PathBuf::from("/music"));
    }

    #[test]
    fn a_bad_playlist_name_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryDirs::under(directory.path());

        let refused = load_named_playlist(Startup::default(), &library, "..");

        assert!(matches!(refused, Err(Error::PlaylistName(_))));
    }

    #[test]
    fn a_saved_playlist_sets_the_tracks_index_and_source() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryDirs::under(directory.path());
        let playlists = directory.path().join("playlists");
        std::fs::create_dir(&playlists).unwrap();
        std::fs::write(playlists.join("fav.m3u8"), "#EXTM3U\na.flac\nb.flac\n")
            .unwrap();
        let saved =
            library::playlists::load(&library, &PlaylistFileName::new("fav").unwrap())
                .unwrap();

        let startup = load_named_playlist(Startup::default(), &library, "fav").unwrap();

        let paths: Vec<_> = startup
            .playlist_tracks
            .iter()
            .map(|track| track.path().to_path_buf())
            .collect();
        assert_eq!(paths, [playlists.join("a.flac"), playlists.join("b.flac")]);
        assert_eq!(startup.playlist_index, saved.playing_index());
        assert_eq!(startup.playlist_source, PlaylistSource::Named);
    }
}
