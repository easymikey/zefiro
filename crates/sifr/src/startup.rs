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
    parse_appearance,
    parse_config,
};
use kernel::{
    Bounded,
    Track,
    domain::{
        CustomSetting,
        DeviceName,
        Percent,
        PlaylistIndex,
        Shuffle,
        Startup,
        ThemeChoice,
    },
    playlist::{PlaylistFileName, PlaylistSource},
};
use library::LibraryPaths;

use crate::error::Error;

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

fn read_config_file(path: Option<&Path>) -> Result<ConfigFile, Error> {
    let Some(path) = path else {
        return Ok(ConfigFile::default());
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text).map_err(|source| Error::ConfigParse {
            path: path.to_path_buf(),
            source,
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(ConfigFile::default())
        }
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

fn config_paths(root: Option<&Path>, config: Option<PathBuf>) -> runtime::ConfigPaths {
    let themes =
        root.map_or_else(|| PathBuf::from("themes"), |root| root.join("themes"));
    runtime::ConfigPaths {
        config,
        appearance: appearance_path(root),
        themes,
        theme: None,
    }
}

fn appearance_file_at_boot(path: &Path) -> AppearanceFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppearanceFile::default();
    };
    parse_appearance(&text).unwrap_or_default()
}

fn seeded_custom_rows(path: &Path) -> Vec<CustomSetting> {
    config::custom_rows(&appearance_file_at_boot(path))
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
        output_device: file
            .audio
            .device
            .and_then(|name| DeviceName::new(name).ok()),
        sleep_presets: file.audio.sleep_presets.into(),
        theme: overrides
            .theme
            .as_deref()
            .map_or(file.theme, parse_cli_theme),
        volume: overrides.volume.map_or(file.volume, Percent::clamped),
        themes: Vec::new(),
        custom_rows: Vec::new(),
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

pub(crate) fn boot() -> Result<(Startup, runtime::BootPaths), Error> {
    let cli = Cli::parse();
    let root = configuration_root();
    let config = root.as_deref().map(|root| root.join(CONFIG_FILE_NAME));
    let file = read_config_file(config.as_deref())?;
    let music_dir = resolved_music_dir(&file, cli.path)?;
    let library = LibraryPaths::from_dirs()?;
    let playlist = loaded_playlist(&library, cli.playlist.as_deref())?;
    let shuffle = shuffle_requested(cli.shuffle);
    let overrides = Overrides {
        theme: cli.theme,
        volume: cli.volume,
    };
    let appearance = appearance_path(root.as_deref());
    let startup = Startup {
        custom_rows: seeded_custom_rows(&appearance),
        ..with_playlist(
            startup_from_file(file, music_dir, &overrides),
            playlist,
            shuffle,
        )
    };
    let paths = runtime::BootPaths {
        config: config_paths(root.as_deref(), config),
        library,
    };
    Ok((startup, paths))
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use config::AppearanceFile;
    use kernel::domain::Shuffle;
    use rstest::rstest;

    use crate::startup::{Cli, seeded_custom_rows, shuffle_requested};

    fn stock_rows() -> Vec<kernel::domain::CustomSetting> {
        config::custom_rows(&AppearanceFile::default())
    }

    #[test]
    fn a_broken_appearance_file_still_seeds_the_stock_appearance_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sifr-ui.toml");
        std::fs::write(&path, "[volume]\nmode = \"text\"\n").unwrap();

        assert_eq!(seeded_custom_rows(&path), stock_rows());
    }

    #[test]
    fn a_missing_appearance_file_seeds_the_stock_appearance_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sifr-ui.toml");

        assert_eq!(seeded_custom_rows(&path), stock_rows());
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
