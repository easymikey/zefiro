use crate::{
    cmd::{
        AudioCmd,
        Cmd,
        ConfigCmd,
        Cue,
        Effect,
        LibraryCmd,
        MacosCmd,
        Playback,
        PlaybackChange,
        ScanMode,
    },
    domain::{
        ConfigError,
        ConfigName,
        DriverName,
        Model,
        Settings,
        Shuffle,
        Startup,
        Themes,
        playlist::Playlist,
    },
    message::ConfigReload,
    update::{
        machine::{Machine, Unhandled},
        playlist::PlaylistMessage,
    },
};

pub(crate) fn seed_model(model: &mut Model, startup: Startup) -> Cmd {
    let errors = startup.errors;
    model.settings = Settings {
        audio: startup.audio,
        output_devices: Vec::new(),
        appearance: startup.appearance,
    };
    model.transport.volume = startup.volume;
    model.appearance_settings = startup.appearance_settings;

    model.library = None;
    model.music_dir = startup.music_dir;
    model.playlist_source = startup.playlist_source;
    model.themes = Themes {
        names: startup.themes,
        selected: startup.theme,
    };
    model
        .playlist
        .relist(startup.playlist_tracks, startup.playlist_index);
    let cmd = shuffled(&mut model.playlist, startup.shuffle);

    let stopped: Cmd = PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(None))])
        .collect();
    let notices = toast_notices(model, errors);
    cmd.then(stopped)
        .then(startup_cmd(model, DriverName::Audio))
        .then(startup_cmd(model, DriverName::Library))
        .then(startup_cmd(model, DriverName::Config))
        .then(notices)
}

pub(crate) fn startup_cmd(model: &mut Model, driver: DriverName) -> Cmd {
    match driver {
        DriverName::Audio => Cmd::from_iter([
            Effect::Audio(AudioCmd::ListDevices),
            Effect::Audio(AudioCmd::SetDevice(model.settings.audio.device.clone())),
            Effect::Audio(AudioCmd::SetCrossfade(model.settings.audio.crossfade)),
            Effect::Audio(AudioCmd::SetReplayGain(model.settings.audio.replay_gain)),
        ]),
        DriverName::Library => Cmd::from_iter([
            Effect::Library(LibraryCmd::LoadFavorites),
            Effect::Library(LibraryCmd::Scan {
                music_dir: model.music_dir.clone(),
                revision: model.revisions.issue_scan(),
                mode: ScanMode::Cached,
            }),
        ]),
        DriverName::Config => {
            Effect::Config(ConfigCmd::SelectTheme(model.themes.selected.clone())).into()
        }
        DriverName::Macos => Cmd::from_iter([
            Effect::Macos(MacosCmd::NowPlaying(None)),
            Effect::Macos(MacosCmd::SetPlayback(Playback::Paused)),
            Effect::Macos(MacosCmd::SetVolume(model.transport.volume)),
        ]),
    }
}

fn toast_notices(model: &mut Model, errors: Vec<(ConfigName, ConfigError)>) -> Cmd {
    let mut errors = errors.into_iter();
    let Some((name, error)) = errors.next() else {
        return Cmd::none();
    };
    let cmd = model.workspace.config_reloaded(
        ConfigReload {
            name,
            result: Err(error),
        },
        &mut model.revisions,
    );
    errors.for_each(|(rest_name, rest_error)| {
        model
            .workspace
            .config_errors
            .insert_if_changed(rest_name, rest_error);
    });
    cmd
}

fn shuffled(playlist: &mut Playlist, shuffle: Shuffle) -> Cmd {
    match shuffle {
        Shuffle::Disabled => Cmd::none(),
        Shuffle::Enabled => playlist
            .transition(PlaylistMessage::ToggleShuffle)
            .unwrap_or_else(|Unhandled| Cmd::none())
            .then(Cue::PlayOrderChanged.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use crate::{
        cmd::{Effect, LibraryCmd},
        domain::{
            AudioSettings,
            Bounded,
            Crossfade,
            DeviceName,
            Model,
            OutputDevice,
            Percent,
            ReplayGain,
            Shuffle,
            SleepPresets,
            Startup,
            ThemeChoice,
            ThemeName,
            Track,
            TrackRef,
            ViewIndex,
            playlist::{PlayOrder, PlaylistSource},
        },
        update::startup::seed_model,
    };

    fn stock_startup() -> Startup {
        let tracks = vec![
            Arc::new(Track::listed(Path::new("/music/a.flac"))),
            Arc::new(Track::listed(Path::new("/music/b.flac"))),
        ];

        Startup {
            music_dir: PathBuf::from("/music"),
            playlist_tracks: tracks,
            playlist_index: Some(ViewIndex::new(0)),
            playlist_source: PlaylistSource::Named,
            shuffle: Shuffle::Enabled,
            audio: AudioSettings {
                crossfade: Crossfade::clamped(Duration::from_secs(3)),
                replay_gain: ReplayGain::On,
                device: OutputDevice::Named(
                    DeviceName::new("Speakers".to_string()).unwrap(),
                ),
                sleep_presets: SleepPresets::from_minutes(&[15, 30]).unwrap(),
            },
            appearance: crate::domain::appearance::Appearance::default(),
            theme: ThemeChoice::Named(ThemeName::from_static("dark")),
            volume: Percent::clamped(42),
            themes: vec![
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar"),
            ],
            appearance_settings: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn startup_errors_raise_one_toast_with_the_first_error() {
        let mut model = Model::default();
        let broken = crate::domain::ConfigError::Invalid {
            detail: "broken".to_string(),
        };
        let unreadable = crate::domain::ConfigError::Unreadable {
            file: crate::domain::ConfigName::Appearance,
            kind: crate::IoError::Other,
        };
        let startup = Startup {
            errors: vec![
                (
                    crate::domain::ConfigName::Theme(ThemeName::from_static("ghost")),
                    broken.clone(),
                ),
                (crate::domain::ConfigName::Appearance, unreadable.clone()),
            ],
            ..stock_startup()
        };

        drop(seed_model(&mut model, startup));

        let texts: Vec<_> = model
            .workspace
            .toasts
            .iter()
            .map(|toast| (toast.kind, toast.text.clone()))
            .collect();
        assert_eq!(
            texts,
            [(crate::domain::ToastKind::Error, Some(broken.to_string()))]
        );
        assert!(
            !model
                .workspace
                .config_errors
                .insert_if_changed(crate::domain::ConfigName::Appearance, unreadable)
        );
        assert!(!model.workspace.config_errors.insert_if_changed(
            crate::domain::ConfigName::Theme(ThemeName::from_static("ghost")),
            broken
        ));
    }

    #[test]
    fn startup_without_notices_raises_no_toast() {
        let mut model = Model::default();
        drop(seed_model(&mut model, stock_startup()));

        assert!(model.workspace.toasts.is_empty());
    }

    fn startup_model() -> Model {
        let mut model = Model::default();
        drop(seed_model(&mut model, stock_startup()));
        model
    }

    #[test]
    fn startup_seeds_settings_transport_and_themes() {
        let model = startup_model();

        insta::assert_debug_snapshot!((
            model.settings,
            model.transport.volume,
            model.themes,
        ));
    }

    #[test]
    fn startup_leaves_library_loading_and_seeds_the_playlist() {
        let model = startup_model();

        assert!(model.library.is_none());
        assert_eq!(model.playlist.tracks.len(), 2);
        assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(0)));
    }

    #[test]
    fn startup_seeds_music_dir_and_requests_a_library_scan() {
        let mut model = Model::default();
        let cmd = seed_model(&mut model, stock_startup());

        assert_eq!(model.music_dir, PathBuf::from("/music"));
        let (effects, _messages) = cmd.into_parts();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Library(LibraryCmd::Scan { music_dir, .. }) if music_dir == Path::new("/music")
        )));
    }

    #[test]
    fn startup_seeds_shuffle_favorites_and_themes() {
        let model = startup_model();

        assert!(model.settings.output_devices.is_empty());
        assert_eq!(model.playlist.play_order, PlayOrder::ShufflePending);
        assert!(
            !model
                .favorites
                .is_favorite(&TrackRef::Local("/music/a.flac".into()))
        );
        assert_eq!(
            model.themes.names,
            [
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar")
            ]
        );
    }

    #[test]
    fn startup_returns_effects_including_devices_favorites_stop_and_cleared_now_playing()
     {
        let mut model = Model::default();
        let cmd = seed_model(&mut model, stock_startup());

        let (effects, _messages) = cmd.into_parts();
        insta::assert_debug_snapshot!(effects);
    }

    fn idempotence_fields(model: &Model) -> impl std::fmt::Debug + PartialEq {
        (
            model.settings.audio.clone(),
            model.transport.volume,
            model.favorites.clone(),
            model.library.is_none(),
            model.playlist.tracks.len(),
            model.playlist.playing_index(),
            model.themes.clone(),
        )
    }

    #[test]
    fn startup_is_idempotent_for_the_same_startup() {
        let mut first = Model::default();
        drop(seed_model(&mut first, stock_startup()));

        let mut second = Model::default();
        drop(seed_model(&mut second, stock_startup()));
        drop(seed_model(&mut second, stock_startup()));

        let projected = idempotence_fields(&second);
        assert_eq!(idempotence_fields(&first), projected);
        insta::assert_debug_snapshot!(projected);
    }
}
