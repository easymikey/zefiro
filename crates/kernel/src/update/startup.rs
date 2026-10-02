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
        Driver,
        Model,
        Settings,
        Shuffle,
        Startup,
        Themes,
        Toast,
        playlist::Playlist,
    },
    update::playlist::PlaylistMessage,
};

pub(crate) fn seed_model(model: &mut Model, startup: Startup) -> Cmd {
    let toasts = startup.toast_texts;
    model.settings = Settings {
        audio: startup.audio,
        output_devices: Vec::new(),
        look: startup.look,
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

    let effects = PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(None))])
        .collect();
    let notices = toast_notices(model, &toasts);
    cmd.then(Cmd::Batch(effects))
        .then(startup_cmd(model, Driver::Audio))
        .then(startup_cmd(model, Driver::Library))
        .then(startup_cmd(model, Driver::Config))
        .then(notices)
}

pub(crate) fn startup_cmd(model: &mut Model, driver: Driver) -> Cmd {
    match driver {
        Driver::Audio => Cmd::Batch(vec![
            Effect::Audio(AudioCmd::ListDevices),
            Effect::Audio(AudioCmd::SetDevice(model.settings.audio.device.clone())),
            Effect::Audio(AudioCmd::SetCrossfade(model.settings.audio.crossfade)),
            Effect::Audio(AudioCmd::SetReplayGain(model.settings.audio.replay_gain)),
        ]),
        Driver::Library => Cmd::Batch(vec![
            Effect::Library(LibraryCmd::LoadFavorites),
            Effect::Library(LibraryCmd::Scan {
                music_dir: model.music_dir.clone(),
                revision: model.revisions.issue_scan(),
                mode: ScanMode::Cached,
            }),
        ]),
        Driver::Config => {
            Effect::Config(ConfigCmd::SelectTheme(model.themes.selected.clone())).into()
        }
        Driver::Macos => Cmd::Batch(vec![
            Effect::Macos(MacosCmd::NowPlaying(None)),
            Effect::Macos(MacosCmd::PlaybackState(Playback::Paused)),
            Effect::Macos(MacosCmd::Volume(model.transport.volume)),
        ]),
    }
}

fn toast_notices(model: &mut Model, toasts: &[String]) -> Cmd {
    if toasts.is_empty() {
        return Cmd::None;
    }
    model.workspace.show(
        Toast::error("Started with fallbacks").with_text(toasts.join("\n")),
        &mut model.revisions,
    )
}

fn shuffled(playlist: &mut Playlist, shuffle: Shuffle) -> Cmd {
    match shuffle {
        Shuffle::Disabled => Cmd::None,
        Shuffle::Enabled => playlist
            .apply(PlaylistMessage::ToggleShuffle)
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
            look: crate::domain::appearance::Look::default(),
            theme: ThemeChoice::Named(ThemeName::from_static("dark")),
            volume: Percent::clamped(42),
            themes: vec![
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar"),
            ],
            appearance_settings: Vec::new(),
            toast_texts: Vec::new(),
        }
    }

    #[test]
    fn startup_notices_raise_one_error_toast() {
        let mut model = Model::default();
        let startup = Startup {
            toast_texts: vec!["broken a".to_string(), "broken b".to_string()],
            ..stock_startup()
        };

        let _ = seed_model(&mut model, startup);

        assert_eq!(model.workspace.toasts.len(), 1);
        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.kind, crate::domain::ToastKind::Error);
        assert_eq!(toast.text.as_deref(), Some("broken a\nbroken b"));
    }

    #[test]
    fn startup_without_notices_raises_no_toast() {
        let mut model = Model::default();
        let _ = seed_model(&mut model, stock_startup());

        assert!(model.workspace.toasts.is_empty());
    }

    fn startup_model() -> Model {
        let mut model = Model::default();
        let _ = seed_model(&mut model, stock_startup());
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
        let effects: Vec<Effect> = cmd.into_iter().collect();
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
        assert!(!model.favorites.is_favorite(&PathBuf::from("/music/a.flac")));
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

        let effects: Vec<Effect> = cmd.into_iter().collect();
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
        let _ = seed_model(&mut first, stock_startup());

        let mut second = Model::default();
        let _ = seed_model(&mut second, stock_startup());
        let _ = seed_model(&mut second, stock_startup());

        let projected = idempotence_fields(&second);
        assert_eq!(idempotence_fields(&first), projected);
        insta::assert_debug_snapshot!(projected);
    }
}
