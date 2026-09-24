use crate::{
    cmd::{
        AudioCmd,
        Cmd,
        ConfigCmd,
        Cue,
        Effect,
        LibraryCmd,
        NowPlaying,
        PlaybackChange,
        SystemCmd,
    },
    domain::{Loaded, Model, Revision, Settings, Startup, playlist::Playlist},
    update::{machine::Machine, playlist::PlaylistMessage},
};

pub(crate) fn seed_model(model: &mut Model, startup: Startup) -> Cmd {
    let theme = startup.theme.clone();
    model.settings = Settings {
        crossfade: startup.crossfade,
        replaygain: startup.replaygain,
        output_device: startup.output_device,
        output_devices: Vec::new(),
        sleep_presets: startup.sleep_presets,
        theme: startup.theme,
    };
    model.transport.volume = startup.volume;
    model.custom_rows = startup.custom_rows;

    model.library = Loaded::Loading;
    model.music_dir = startup.music_dir.clone();
    model.playlist_source = startup.playlist_source;
    model.themes = startup.themes;
    crate::domain::playlist::relist(
        &mut model.playlist,
        startup.playlist_tracks,
        startup.playlist_index,
    );
    let cmd = startup.shuffle_order.map_or(Cmd::None, |order| {
        shuffle_restored(&mut model.playlist, order)
    });

    let mut effects = PlaybackChange::Stop.effects().to_vec();
    effects.extend([
        Effect::System(SystemCmd::NowPlaying(NowPlaying::default())),
        Effect::Audio(AudioCmd::ListDevices),
        Effect::Library(LibraryCmd::LoadFavorites),
        Effect::Library(LibraryCmd::ScanLibrary {
            root: startup.music_dir,
            revision: Revision::UNSTAMPED,
        }),
        Effect::Config(ConfigCmd::SelectTheme(theme)),
    ]);
    cmd.then(Cmd::Batch(effects))
}

fn shuffle_restored(playlist: &mut Playlist, order: Vec<usize>) -> Cmd {
    let Ok(toggled) = playlist.update(PlaylistMessage::ToggleShuffle);
    let Ok(rolled) = playlist.update(PlaylistMessage::ShuffleRolled(order));
    toggled.then(Cue::PlayOrderChanged.into()).then(rolled)
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
            Bounded,
            Crossfade,
            Model,
            Percent,
            PlaylistIndex,
            Replaygain,
            Startup,
            Track,
            playlist::PlaylistSource,
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
            playlist_index: Some(PlaylistIndex::new(0)),
            playlist_source: PlaylistSource::Named,
            shuffle_order: Some(vec![1, 0]),
            crossfade: Crossfade::clamped(Duration::from_secs(3)),
            replaygain: Replaygain::On,
            output_device: Some("Speakers".to_string()),
            sleep_presets: vec![Duration::from_secs(900), Duration::from_secs(1800)]
                .into(),
            theme: "dark".to_string(),
            volume: Percent::clamped(42),
            themes: vec!["noir".to_string(), "solar".to_string()],
            custom_rows: Vec::new(),
        }
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

        assert!(model.library.is_loading());
        assert_eq!(model.playlist.tracks.len(), 2);
        assert_eq!(model.playlist.anchor(), Some(PlaylistIndex::new(0)));
    }

    #[test]
    fn startup_seeds_music_dir_and_requests_a_library_scan() {
        let mut model = Model::default();
        let cmd = seed_model(&mut model, stock_startup());

        assert_eq!(model.music_dir, PathBuf::from("/music"));
        let effects: Vec<Effect> = cmd.into_iter().collect();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Library(LibraryCmd::ScanLibrary { root, .. }) if root == Path::new("/music")
        )));
    }

    #[test]
    fn startup_seeds_shuffle_favorites_and_themes() {
        let model = startup_model();

        assert!(model.settings.output_devices.is_empty());
        assert!(model.playlist.play_order.is_shuffle());
        assert!(!model.favorites.is_favorite(&PathBuf::from("/music/a.flac")));
        assert_eq!(model.themes, ["noir".to_string(), "solar".to_string()]);
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
            model.settings.crossfade,
            model.settings.replaygain,
            model.settings.output_device.clone(),
            model.settings.sleep_presets.clone(),
            model.settings.theme.clone(),
            model.transport.volume,
            model.favorites.clone(),
            model.library.is_loading(),
            model.playlist.tracks.len(),
            model.playlist.anchor(),
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
