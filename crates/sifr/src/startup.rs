use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Parser;
use config::{
    APPEARANCE_FILE_NAME,
    AppearanceFile,
    CONFIG_FILE_NAME,
    ConfigFile,
    ThemeFile,
    parse_appearance,
    parse_config,
};
use kernel::{
    Bounded,
    Track,
    domain::{Percent, PlaylistIndex, Shuffle, Startup, ThemeChoice},
    playlist::{PlaylistFileName, PlaylistSource},
};
use library::LibraryPaths;

use crate::{error::Error, shell::fallback_theme_file};

pub(crate) struct Boot {
    pub(crate) startup: Startup,
    pub(crate) paths: runtime::BootPaths,
    pub(crate) look: BootLook,
}

#[derive(Debug, Clone)]
pub(crate) struct BootLook {
    pub(crate) theme: ThemeFile,
    pub(crate) appearance: AppearanceFile,
}

struct Observed<T> {
    value: T,
    text: Option<String>,
    notice: Option<String>,
}

struct Observation {
    appearance: Observed<AppearanceFile>,
    theme: Observed<ThemeFile>,
}

struct ConfigRead {
    file: ConfigFile,
    text: Option<String>,
}

#[derive(Debug, Parser)]
#[command(name = "sifr", about = "A terminal music player")]
pub(crate) struct Cli {
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

fn configuration_root() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("sifr"))
}

fn read_config_file(path: Option<&Path>) -> Result<ConfigRead, Error> {
    let absent = ConfigRead {
        file: ConfigFile::default(),
        text: None,
    };
    let Some(path) = path else {
        return Ok(absent);
    };
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_config(&text) {
            Ok(file) => Ok(ConfigRead {
                file,
                text: Some(text),
            }),
            Err(source) => Err(Error::ConfigParse {
                path: path.to_path_buf(),
                source,
            }),
        },
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(absent),
        Err(source) => Err(Error::ConfigRead {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn appearance_path(root: Option<&Path>) -> PathBuf {
    root.map_or_else(
        || PathBuf::from(APPEARANCE_FILE_NAME),
        |root| root.join(APPEARANCE_FILE_NAME),
    )
}

fn themes_path(root: Option<&Path>) -> PathBuf {
    root.map_or_else(|| PathBuf::from("themes"), |root| root.join("themes"))
}

fn config_paths(
    root: Option<&Path>,
    choice: &ThemeChoice,
    config: (Option<PathBuf>, runtime::SeenTexts),
) -> runtime::ConfigPaths {
    let (config, seen) = config;
    runtime::ConfigPaths {
        config,
        appearance: appearance_path(root),
        themes: themes_path(root),
        theme: Some(config::resolve_theme(choice).to_string()),
        seen,
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source),
    }
}

fn unreadable<T>(value: T, path: &Path, source: &std::io::Error) -> Observed<T> {
    Observed {
        value,
        text: None,
        notice: Some(format!("cannot read {}: {source}", path.display())),
    }
}

fn observed_appearance(path: &Path) -> Observed<AppearanceFile> {
    let text = match read_optional(path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return Observed {
                value: AppearanceFile::default(),
                text: None,
                notice: None,
            };
        }
        Err(source) => return unreadable(AppearanceFile::default(), path, &source),
    };
    match parse_appearance(&text) {
        Ok(value) => Observed {
            value,
            text: Some(text),
            notice: None,
        },
        Err(error) => Observed {
            value: AppearanceFile::default(),
            text: Some(text),
            notice: Some(error.to_string()),
        },
    }
}

fn stock_theme() -> ThemeFile {
    config::embedded_theme("noir")
        .and_then(|source| config::parse_theme(source, "noir").ok())
        .unwrap_or_else(fallback_theme_file)
}

fn parsed_theme(name: &str, text: &str, source: Option<String>) -> Observed<ThemeFile> {
    match config::parse_theme(text, name) {
        Ok(value) => Observed {
            value,
            text: source,
            notice: None,
        },
        Err(error) => Observed {
            value: stock_theme(),
            text: source,
            notice: Some(error.to_string()),
        },
    }
}

fn missing_theme(name: &str) -> Observed<ThemeFile> {
    config::embedded_theme(name).map_or_else(
        || Observed {
            value: stock_theme(),
            text: None,
            notice: Some(format!("no theme named `{name}`")),
        },
        |embedded| parsed_theme(name, embedded, None),
    )
}

fn observed_theme(choice: &ThemeChoice, themes: &Path) -> Observed<ThemeFile> {
    let resolved = config::resolve_theme(choice);
    let name = resolved.as_str();
    let path = themes.join(config::theme_file_name(name));
    match read_optional(&path) {
        Ok(Some(text)) => parsed_theme(name, &text, Some(text.clone())),
        Ok(None) => missing_theme(name),
        Err(source) => unreadable(stock_theme(), &path, &source),
    }
}

fn observe(root: Option<&Path>, choice: &ThemeChoice) -> Observation {
    Observation {
        appearance: observed_appearance(&appearance_path(root)),
        theme: observed_theme(choice, &themes_path(root)),
    }
}

fn seeded(
    startup: Startup,
    observation: Observation,
    paths: runtime::BootPaths,
) -> Boot {
    let Observation { appearance, theme } = observation;
    let custom_rows = config::custom_rows(&appearance.value);
    let notices = [appearance.notice, theme.notice]
        .into_iter()
        .flatten()
        .collect();
    let mut paths = paths;
    paths.config.seen.appearance = appearance.text;
    paths.config.seen.theme = theme.text;
    Boot {
        startup: Startup {
            custom_rows,
            notices,
            ..startup
        },
        paths,
        look: BootLook {
            theme: theme.value,
            appearance: appearance.value,
        },
    }
}

fn resolved_music_dir(
    file: &ConfigFile,
    override_path: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    let music_dir = override_path
        .or_else(|| file.music_dir.clone())
        .or_else(dirs::audio_dir)
        .ok_or(Error::MusicDirectoryUnset)?;
    if music_dir.is_dir() {
        Ok(music_dir)
    } else {
        Err(Error::MusicDirectoryMissing { path: music_dir })
    }
}

struct LoadedPlaylist {
    tracks: Vec<Arc<Track>>,
    index: Option<PlaylistIndex>,
    source: PlaylistSource,
}

impl LoadedPlaylist {
    fn none() -> Self {
        Self {
            tracks: Vec::new(),
            index: None,
            source: PlaylistSource::Library,
        }
    }
}

fn loaded_playlist(
    library: &LibraryPaths,
    name: Option<&str>,
) -> Result<LoadedPlaylist, Error> {
    let Some(name) = name else {
        return Ok(LoadedPlaylist::none());
    };
    let file_name =
        PlaylistFileName::new(name).map_err(|source| Error::PlaylistName {
            name: name.to_owned(),
            source,
        })?;
    let playlist = library::load_playlist(library, &file_name)?;
    Ok(LoadedPlaylist {
        index: playlist.anchor(),
        tracks: playlist.tracks,
        source: PlaylistSource::Named,
    })
}

struct Overrides {
    theme: Option<String>,
    volume: Option<u8>,
}

fn parse_cli_theme(raw: &str) -> ThemeChoice {
    raw.parse().unwrap_or(ThemeChoice::Auto)
}

fn startup_from_file(
    file: ConfigFile,
    music_dir: PathBuf,
    overrides: &Overrides,
) -> Startup {
    Startup {
        music_dir,
        playlist_tracks: Vec::new(),
        playlist_index: None,
        playlist_source: PlaylistSource::Library,
        shuffle: Shuffle::Disabled,
        crossfade: file.audio.crossfade,
        replaygain: file.audio.replaygain,
        output_device: file.audio.device,
        sleep_presets: file.audio.sleep_presets.into(),
        theme: overrides
            .theme
            .as_deref()
            .map_or(file.theme, parse_cli_theme),
        volume: overrides.volume.map_or(file.volume, Percent::clamped),
        themes: Vec::new(),
        custom_rows: Vec::new(),
        notices: Vec::new(),
    }
}

fn with_playlist(
    mut startup: Startup,
    playlist: LoadedPlaylist,
    shuffle: Shuffle,
) -> Startup {
    startup.shuffle = shuffle;
    startup.playlist_tracks = playlist.tracks;
    startup.playlist_index = playlist.index;
    startup.playlist_source = playlist.source;
    startup
}

pub(crate) fn boot() -> Result<Boot, Error> {
    let cli = Cli::parse();
    let root = configuration_root();
    let config = root.as_deref().map(|root| root.join(CONFIG_FILE_NAME));
    let ConfigRead { file, text: keys } = read_config_file(config.as_deref())?;
    let music_dir = resolved_music_dir(&file, cli.path)?;
    let library = LibraryPaths::from_dirs()?;
    let playlist = loaded_playlist(&library, cli.playlist.as_deref())?;
    let shuffle = shuffle_requested(cli.shuffle);
    let overrides = Overrides {
        theme: cli.theme,
        volume: cli.volume,
    };
    let startup = with_playlist(
        startup_from_file(file, music_dir, &overrides),
        playlist,
        shuffle,
    );
    let observation = observe(root.as_deref(), &startup.theme);
    let seen = runtime::SeenTexts {
        keys,
        ..runtime::SeenTexts::default()
    };
    let paths = runtime::BootPaths {
        config: config_paths(root.as_deref(), &startup.theme, (config, seen)),
        library,
    };
    Ok(seeded(startup, observation, paths))
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use config::AppearanceFile;
    use kernel::domain::{Shuffle, Startup, ThemeChoice, ThemeName};
    use rstest::rstest;

    use crate::startup::{Boot, Cli, observe, seeded, shuffle_requested};

    const COMPACT: &str = "[layout]\nmode = \"compact\"\n";
    const BROKEN: &str = "[volume]\nmode = \"text\"\n";

    fn choice(name: &str) -> ThemeChoice {
        ThemeChoice::Named(ThemeName::new(name.to_string()).unwrap())
    }

    fn booted(directory: &std::path::Path, theme: &ThemeChoice) -> Boot {
        let observation = observe(Some(directory), theme);
        let paths = runtime::BootPaths {
            config: crate::startup::config_paths(
                Some(directory),
                theme,
                (None, runtime::SeenTexts::default()),
            ),
            library: library::LibraryPaths::from_dirs().unwrap(),
        };
        seeded(Startup::default(), observation, paths)
    }

    #[rstest]
    #[case::valid(Some(COMPACT), "noir", &[])]
    #[case::broken(Some(BROKEN), "noir", &["sifr-ui.toml"])]
    #[case::missing(None, "noir", &[])]
    #[case::missing_theme(None, "ghost", &["ghost"])]
    fn boot_look_rows(
        #[case] appearance: Option<&str>,
        #[case] theme: &str,
        #[case] notices: &[&str],
    ) {
        let directory = tempfile::tempdir().unwrap();
        if let Some(text) = appearance {
            std::fs::write(directory.path().join("sifr-ui.toml"), text).unwrap();
        }

        let boot = booted(directory.path(), &choice(theme));

        let expected = if appearance == Some(COMPACT) {
            config::parse_appearance(COMPACT).unwrap()
        } else {
            AppearanceFile::default()
        };
        assert_eq!(boot.look.appearance, expected);
        assert_eq!(boot.startup.notices.len(), notices.len());
        for (notice, fragment) in boot.startup.notices.iter().zip(notices) {
            assert!(notice.contains(fragment), "{notice} lacks {fragment}");
        }
        assert!(!boot.look.theme.name.is_empty());
    }

    #[test]
    fn an_auto_theme_boots_as_noir_and_is_watched() {
        let directory = tempfile::tempdir().unwrap();

        let boot = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(boot.look.theme.name, "noir");
        assert_eq!(boot.paths.config.theme.as_deref(), Some("noir"));
    }

    #[test]
    fn a_named_theme_is_watched_under_its_name() {
        let directory = tempfile::tempdir().unwrap();

        let boot = booted(directory.path(), &choice("ghost"));

        assert_eq!(boot.paths.config.theme.as_deref(), Some("ghost"));
    }

    #[test]
    fn seen_texts_carry_the_boot_texts() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();
        std::fs::create_dir(directory.path().join("themes")).unwrap();
        let theme = config::embedded_theme("noir").unwrap();
        std::fs::write(directory.path().join("themes/mine.toml"), theme).unwrap();

        let boot = booted(directory.path(), &choice("mine"));

        let seen = boot.paths.config.seen;
        assert_eq!(seen.appearance.as_deref(), Some(COMPACT));
        assert_eq!(seen.theme.as_deref(), Some(theme));
    }

    #[test]
    fn the_custom_rows_and_the_shell_see_the_same_appearance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sifr-ui.toml"), COMPACT).unwrap();

        let boot = booted(directory.path(), &ThemeChoice::Auto);

        assert_eq!(
            boot.startup.custom_rows,
            config::custom_rows(&boot.look.appearance)
        );
        assert_ne!(
            boot.startup.custom_rows,
            config::custom_rows(&AppearanceFile::default())
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
