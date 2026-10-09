use std::path::{Path, PathBuf};

use clap::{
    Parser,
    builder::{BoolValueParser, PathBufValueParser, TypedValueParser},
};
use config::{
    config_file::TomlSettings,
    driver::paths::{ConfigPaths, SeenTexts},
    embedded_theme::{STOCK_THEME, STOCK_THEME_TEXT, theme_name},
    file_name::CONFIG_FILE_NAME,
    load::{Loaded, load},
    theme_file::{TomlTheme, parse_theme},
};
use kernel::{
    cmd::ConfigPatch,
    domain::{
        bounded::Bounded,
        overlay::Verdict,
        percent::Percent,
        playlist::{PlaylistFileName, PlaylistSource},
        server::Account,
        startup::{Shuffle, Startup},
        theme::ThemeChoice,
    },
};
use library::{dirs::LibraryDirs, scan::probe};

use crate::error::Error;

pub(crate) struct Launch {
    pub(crate) startup: Startup,
    pub(crate) paths: runtime::spawn_setup::StartupPaths,
    pub(crate) theme: TomlTheme,
    pub(crate) appearance: kernel::domain::appearance::Appearance,
}

#[derive(Debug, Parser)]
#[command(name = "zefiro", version, about = "A terminal music player")]
struct Cli {
    #[arg(help = "Play this folder for this run only, without saving it")]
    path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "path",
        value_parser = PathBufValueParser::new().try_map(checked),
        help = "Save this folder as music_dir in config.toml and start on it"
    )]
    music_dir: Option<PathBuf>,

    #[arg(
        long,
        help = "Use this theme for this run only: auto, an embedded theme or a themes/<name>.toml file"
    )]
    theme: Option<String>,

    #[arg(
        long,
        value_parser = clap::value_parser!(u8).range(0..=100),
        help = "Start this run at this volume, 0 to 100"
    )]
    volume: Option<u8>,

    #[arg(
        long,
        action = clap::ArgAction::SetTrue,
        value_parser = BoolValueParser::new().map(|on| if on { Shuffle::On } else { Shuffle::Off }),
        help = "Start this run with shuffle on"
    )]
    shuffle: Shuffle,

    #[arg(
        long,
        help = "Start this run on this saved playlist, named without .m3u8"
    )]
    playlist: Option<String>,
}

fn checked(path: PathBuf) -> Result<PathBuf, String> {
    match probe(&path) {
        Verdict::Readable => {
            std::path::absolute(path).map_err(|error| error.to_string())
        }
        verdict @ (Verdict::Missing
        | Verdict::NotADirectory
        | Verdict::Denied
        | Verdict::Unreadable(_)) => Err(verdict.to_string()),
    }
}

fn user_config_dir() -> Result<PathBuf, Error> {
    dirs::config_dir()
        .map(|directory| directory.join("zefiro"))
        .ok_or(Error::ConfigDirUnset)
}

fn cli_theme(cli: &Cli) -> Result<Option<ThemeChoice>, Error> {
    cli.theme
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(Error::ThemeName)
}

fn config_paths(config_dir: &Path, theme_choice: Option<&ThemeChoice>) -> ConfigPaths {
    ConfigPaths {
        config_path: config_dir.join(CONFIG_FILE_NAME),
        themes_dir: config_dir.join("themes"),
        default_music_dir: dirs::audio_dir(),
        theme_name: theme_choice.map(theme_name),
        seen_texts: SeenTexts::default(),
    }
}

fn stock_theme() -> Result<TomlTheme, Error> {
    parse_theme(STOCK_THEME_TEXT, STOCK_THEME).map_err(Error::StockTheme)
}

fn resolved_music_dir(
    toml_settings: &TomlSettings,
    cli_path: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    let music_dir = cli_path
        .or_else(|| toml_settings.music_dir.clone())
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
    library_dirs: &LibraryDirs,
    name: &str,
) -> Result<Startup, Error> {
    let file_name = PlaylistFileName::new(name).map_err(Error::PlaylistName)?;
    let playlist = library::playlists::load(library_dirs, &file_name)?;
    Ok(Startup {
        playlist_index: playlist.playing_index(),
        playlist_tracks: playlist.tracks,
        playlist_source: PlaylistSource::Named,
        ..startup
    })
}

fn merged_startup(
    toml_settings: TomlSettings,
    music_dir: PathBuf,
    cli: &Cli,
) -> Startup {
    Startup {
        music_dir,
        shuffle: cli.shuffle,
        keymap_overrides: toml_settings.keymap.into_keymap_overrides(),
        accounts: toml_settings
            .servers
            .into_iter()
            .map(Account::from)
            .collect(),
        audio_settings: toml_settings.audio.into(),
        theme_choice: toml_settings.theme_choice,
        volume: cli.volume.map_or(toml_settings.volume, Percent::clamped),
        ..Startup::default()
    }
}

fn start(
    cli: &Cli,
    config_dir: &Path,
    library_dirs: LibraryDirs,
) -> Result<Launch, Error> {
    let theme_choice = cli_theme(cli)?;
    let paths = config_paths(config_dir, theme_choice.as_ref());
    let Loaded {
        toml_settings,
        theme_name,
        toml_theme,
        texts,
        errors,
    } = load(
        &paths,
        cli.music_dir.clone().map(|music_dir| ConfigPatch {
            music_dir: Some(music_dir),
            ..ConfigPatch::default()
        }),
    );
    let appearance_settings = toml_settings.to_appearance_settings();
    let appearance = toml_settings.to_appearance();
    let music_dir = resolved_music_dir(
        &toml_settings,
        cli.music_dir.as_ref().or(cli.path.as_ref()).cloned(),
    )?;
    let merged = merged_startup(toml_settings, music_dir, cli);
    let themed_startup = Startup {
        theme_choice: theme_choice.unwrap_or(merged.theme_choice),
        ..merged
    };
    let startup = match cli.playlist.as_deref() {
        Some(name) => load_named_playlist(themed_startup, &library_dirs, name)?,
        None => themed_startup,
    };
    let theme = match toml_theme {
        Some(theme) => theme,
        None => stock_theme()?,
    };
    Ok(Launch {
        startup: Startup {
            appearance_settings,
            errors,
            ..startup
        },
        paths: runtime::spawn_setup::StartupPaths {
            config_paths: ConfigPaths {
                theme_name: Some(theme_name),
                seen_texts: texts,
                ..paths
            },
            library_dirs,
        },
        theme,
        appearance,
    })
}

pub(crate) fn launch() -> Result<Launch, Error> {
    start(&Cli::parse(), &user_config_dir()?, LibraryDirs::user()?)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        path::{Path, PathBuf},
    };

    use clap::{CommandFactory, Parser, error::ErrorKind};
    use config::{
        config_file::{TomlSettings, parse_config},
        patch::patched_config_text,
    };
    use kernel::{
        cmd::ConfigPatch,
        domain::{
            bounded::Bounded,
            config::{ConfigError, ConfigName},
            device::{DeviceName, OutputDevice},
            keymap::{Action, KeyOverride, KeymapOverrides},
            percent::Percent,
            playlist::{PlaylistFileName, PlaylistSource},
            settings::AudioSettings,
            startup::{Shuffle, Startup},
            theme::{ThemeChoice, ThemeName},
        },
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
            start,
        },
    };

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn launched(dir: &Path, theme: Option<&str>) -> Launch {
        let cli = Cli {
            path: Some(dir.to_path_buf()),
            music_dir: None,
            theme: theme.map(str::to_owned),
            volume: None,
            shuffle: Shuffle::Off,
            playlist: None,
        };
        start(&cli, dir, library_dirs(dir)).unwrap()
    }

    fn library_dirs(dir: &Path) -> LibraryDirs {
        LibraryDirs::new(&dir.join("cache"), &dir.join("data"), &dir.join("config"))
    }

    #[rstest]
    #[case::valid(Some(COMPACT), "noir", vec![])]
    #[case::broken(Some(BROKEN), "noir", vec![ConfigName::Config])]
    #[case::missing(None, "noir", vec![])]
    #[case::embedded_or_stock_theme(
        None,
        "ghost",
        vec![ConfigName::Theme(ThemeName::from_static("ghost"))]
    )]
    fn a_broken_appearance_or_missing_theme_falls_back_and_reports_why(
        #[case] appearance: Option<&str>,
        #[case] theme: &str,
        #[case] failed_config_names: Vec<ConfigName>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        if let Some(text) = appearance {
            std::fs::write(directory.path().join("config.toml"), text).unwrap();
        }

        let launched = launched(directory.path(), Some(theme));

        let expected = if appearance == Some(COMPACT) {
            parse_config(COMPACT).unwrap()
        } else {
            TomlSettings::default()
        };
        assert_eq!(
            launched.startup.appearance_settings,
            expected.to_appearance_settings()
        );
        assert_eq!(launched.appearance, expected.to_appearance());
        assert!(
            launched
                .startup
                .errors
                .iter()
                .all(|(_, error)| matches!(error, ConfigError::Parse(_))),
            "{:?}",
            launched.startup.errors
        );
        let names: Vec<_> = launched
            .startup
            .errors
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        assert_eq!(names, failed_config_names);
        assert!(!launched.theme.name.as_str().is_empty());
    }

    #[rstest]
    #[case::auto(None, "noir")]
    #[case::named(Some("ghost"), "ghost")]
    fn the_theme_is_watched_under_its_name(
        #[case] flag: Option<&str>,
        #[case] watched: &str,
    ) {
        let directory = tempfile::tempdir().unwrap();

        let launched = launched(directory.path(), flag);

        assert_eq!(launched.theme.name.as_str(), "noir");
        assert_eq!(
            launched
                .paths
                .config_paths
                .theme_name
                .as_ref()
                .map(ThemeName::as_str),
            Some(watched)
        );
    }

    #[rstest]
    #[case::flag(Some("noir"), "noir")]
    #[case::no_flag(None, "ghost")]
    fn a_cli_theme_wins_over_the_config_theme(
        #[case] flag: Option<&str>,
        #[case] expected: &'static str,
    ) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "theme = \"ghost\"\n")
            .unwrap();

        let launched = launched(directory.path(), flag);

        assert_eq!(
            launched.startup.theme_choice,
            ThemeChoice::Named(ThemeName::from_static(expected))
        );
    }

    #[test]
    fn the_first_start_writes_the_template_and_starts_on_the_defaults() {
        let directory = tempfile::tempdir().unwrap();

        let launched = launched(directory.path(), None);

        let written =
            std::fs::read_to_string(directory.path().join("config.toml")).unwrap();
        assert_eq!(written, include_str!("../../config/config.toml"));
        let toml_settings = TomlSettings::default();
        assert_eq!(parse_config(&written).unwrap(), toml_settings);
        assert_eq!(launched.startup.errors, []);
        assert_eq!(launched.startup.volume, toml_settings.volume);
        assert_eq!(launched.startup.theme_choice, toml_settings.theme_choice);
        assert_eq!(
            launched.startup.appearance_settings,
            toml_settings.to_appearance_settings()
        );
        assert_eq!(launched.appearance, toml_settings.to_appearance());
    }

    #[test]
    fn seen_texts_carry_the_launch_texts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), COMPACT).unwrap();
        std::fs::create_dir(directory.path().join("themes")).unwrap();
        let theme = config::embedded_theme::embedded_theme("noir").unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), theme).unwrap();

        let launched = launched(directory.path(), Some("mine"));

        let seen = launched.paths.config_paths.seen_texts;
        assert_eq!(seen.config.as_deref(), Some(COMPACT));
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
            launched.startup.keymap_overrides,
            KeymapOverrides::from([(Action::Quit, KeyOverride::from("q"))])
        );
    }

    #[test]
    fn volume_above_the_maximum_is_rejected() {
        let error = Cli::try_parse_from(["zefiro", "--volume", "150"]).unwrap_err();

        assert!(error.to_string().contains("volume"));
    }

    struct CliRow {
        flags: &'static [&'static str],
        path: Option<&'static str>,
        theme: Option<&'static str>,
        volume: Option<u8>,
        shuffle: Shuffle,
        playlist: Option<&'static str>,
    }

    #[rstest]
    #[case::no_flag(CliRow {
        flags: &["zefiro"],
        path: None,
        theme: None,
        volume: None,
        shuffle: Shuffle::Off,
        playlist: None,
    })]
    #[case::every_flag(CliRow {
        flags: &[
            "zefiro",
            "/music",
            "--theme",
            "noir",
            "--volume",
            "42",
            "--shuffle",
            "--playlist",
            "favourites",
        ],
        path: Some("/music"),
        theme: Some("noir"),
        volume: Some(42),
        shuffle: Shuffle::On,
        playlist: Some("favourites"),
    })]
    fn every_flag_parses_into_its_field(#[case] row: CliRow) {
        let cli = Cli::try_parse_from(row.flags).unwrap();

        assert_eq!(cli.path.as_deref(), row.path.map(Path::new));
        assert_eq!(cli.theme.as_deref(), row.theme);
        assert_eq!(cli.volume, row.volume);
        assert_eq!(cli.shuffle, row.shuffle);
        assert_eq!(cli.playlist.as_deref(), row.playlist);
    }

    #[test]
    fn the_cli_music_dir_wins_over_the_config() {
        let cli_dir = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let settings =
            parse_config(&format!("music_dir = {:?}\n", config_dir.path())).unwrap();

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
    fn a_music_dir_flag_saves_the_folder_in_the_config_and_starts_on_it() {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let cli = Cli::try_parse_from([
            OsStr::new("zefiro"),
            OsStr::new("--music-dir"),
            music.as_os_str(),
        ])
        .unwrap();

        let launched =
            start(&cli, directory.path(), library_dirs(directory.path())).unwrap();

        let written =
            std::fs::read_to_string(directory.path().join("config.toml")).unwrap();
        let expected = patched_config_text(
            include_str!("../../config/config.toml"),
            ConfigPatch {
                music_dir: Some(music.clone()),
                ..ConfigPatch::default()
            },
        )
        .unwrap();
        assert_eq!(written, expected);
        assert_eq!(launched.startup.music_dir, music);
        assert_eq!(launched.paths.config_paths.seen_texts.config, Some(written));
    }

    #[test]
    fn a_relative_music_dir_is_saved_absolute() {
        let directory = tempfile::tempdir().unwrap();
        let cli = Cli::try_parse_from(["zefiro", "--music-dir", "src"]).unwrap();

        let launched =
            start(&cli, directory.path(), library_dirs(directory.path())).unwrap();

        let absolute = std::env::current_dir().unwrap().join("src");
        let written =
            std::fs::read_to_string(directory.path().join("config.toml")).unwrap();
        assert_eq!(
            parse_config(&written).unwrap().music_dir,
            Some(absolute.clone())
        );
        assert_eq!(launched.startup.music_dir, absolute);
    }

    #[rstest]
    #[case::missing("gone", "no such path")]
    #[case::a_file("file", "not a folder")]
    fn a_music_dir_that_is_not_a_readable_folder_is_refused_before_start(
        #[case] name: &str,
        #[case] reason: &str,
    ) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("file"), "").unwrap();
        let path = directory.path().join(name);

        let refused = Cli::try_parse_from([
            OsStr::new("zefiro"),
            OsStr::new("--music-dir"),
            path.as_os_str(),
        ])
        .unwrap_err();

        assert_eq!(refused.kind(), ErrorKind::ValueValidation);
        assert_eq!(refused.exit_code(), 2);
        assert!(refused.to_string().contains(reason), "{refused}");
        assert!(!directory.path().join("config.toml").exists());
    }

    #[test]
    fn the_help_names_the_run_only_path_and_the_saved_music_dir() {
        let help = Cli::command().render_long_help().to_string();

        assert!(
            help.contains("Play this folder for this run only, without saving it"),
            "{help}"
        );
        assert!(help.contains("--music-dir <PATH>"), "{help}");
        assert!(
            help.contains(
                "Save this folder as music_dir in config.toml and start on it"
            ),
            "{help}"
        );
    }

    #[rstest]
    #[case::theme(
        "--theme <THEME>",
        "Use this theme for this run only: auto, an embedded theme or a themes/<name>.toml file"
    )]
    #[case::volume("--volume <VOLUME>", "Start this run at this volume, 0 to 100")]
    #[case::shuffle("--shuffle\n", "Start this run with shuffle on")]
    #[case::playlist(
        "--playlist <PLAYLIST>",
        "Start this run on this saved playlist, named without .m3u8"
    )]
    #[case::version("--version", "Print version")]
    fn the_help_describes_every_option(#[case] usage: &str, #[case] description: &str) {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains(usage), "{help}");
        assert!(help.contains(description), "{help}");
    }

    #[test]
    fn version_prints_the_name_and_the_package_version() {
        let shown = Cli::try_parse_from(["zefiro", "--version"]).unwrap_err();

        assert_eq!(shown.kind(), ErrorKind::DisplayVersion);
        assert_eq!(
            shown.to_string(),
            format!("zefiro {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_bad_cli_theme_name_is_refused() {
        let cli = Cli::try_parse_from(["zefiro", "--theme", ""]).unwrap();

        let refused = cli_theme(&cli);

        assert!(matches!(refused, Err(Error::ThemeName(_))));
    }

    #[rstest]
    #[case::the_cli_volume_wins(
        &["zefiro", "--volume", "80"],
        "volume = 10\n",
        Startup { volume: Percent::clamped(80), ..Startup::default() }
    )]
    #[case::the_config_volume_otherwise(
        &["zefiro"],
        "volume = 10\n",
        Startup { volume: Percent::clamped(10), ..Startup::default() }
    )]
    #[case::the_cli_shuffle_and_the_config_audio(
        &["zefiro", "--shuffle"],
        "volume = 10\n[audio]\ndevice = \"Speakers\"\n",
        Startup {
            volume: Percent::clamped(10),
            shuffle: Shuffle::On,
            audio_settings: AudioSettings {
                device: OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap()),
                ..AudioSettings::default()
            },
            ..Startup::default()
        }
    )]
    fn the_volume_and_shuffle_come_from_the_cli_before_the_config_and_the_audio_from_the_config(
        #[case] flags: &[&str],
        #[case] config_text: &str,
        #[case] expected: Startup,
    ) {
        let cli = Cli::try_parse_from(flags).unwrap();
        let settings = parse_config(config_text).unwrap();

        let merged = merged_startup(settings, PathBuf::from("/music"), &cli);

        assert_eq!(merged.volume, expected.volume);
        assert_eq!(merged.music_dir, PathBuf::from("/music"));
        assert_eq!(merged.shuffle, expected.shuffle);
        assert_eq!(merged.audio_settings, expected.audio_settings);
    }

    #[test]
    fn a_bad_playlist_name_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let library = library_dirs(directory.path());

        let refused = load_named_playlist(Startup::default(), &library, "..");

        assert!(matches!(refused, Err(Error::PlaylistName(_))));
    }

    #[test]
    fn a_saved_playlist_sets_the_tracks_index_and_source() {
        let directory = tempfile::tempdir().unwrap();
        let library = library_dirs(directory.path());
        let playlists = directory
            .path()
            .join("config")
            .join("zefiro")
            .join("playlists");
        std::fs::create_dir_all(&playlists).unwrap();
        std::fs::write(playlists.join("fav.m3u8"), "#EXTM3U\na.flac\nb.flac\n")
            .unwrap();
        let saved =
            library::playlists::load(&library, &PlaylistFileName::new("fav").unwrap())
                .unwrap();

        let startup = load_named_playlist(Startup::default(), &library, "fav").unwrap();

        let paths: Vec<_> = startup
            .playlist_tracks
            .iter()
            .map(|track| track.local_path().unwrap().to_path_buf())
            .collect();
        assert_eq!(paths, [playlists.join("a.flac"), playlists.join("b.flac")]);
        assert_eq!(startup.playlist_index, saved.playing_index());
        assert_eq!(startup.playlist_source, PlaylistSource::Named);
    }
}
