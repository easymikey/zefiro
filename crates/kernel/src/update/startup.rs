use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, DiskCmd, Effect, LibraryCmd, ScanMode},
    domain::{
        config::{ConfigError, ConfigName},
        driver::DriverName,
        model::Model,
        playlist::PlayOrder,
        settings::Settings,
        startup::{Shuffle, Startup},
        theme::Themes,
    },
    update::{
        drained,
        keymap::bindings::Keymap,
        player::stopped_effects,
        roll_pending,
        workspace::trouble,
    },
};

#[must_use]
pub fn startup(startup: Startup) -> (Model, Vec<Effect>) {
    let mut model = Model::default();
    let cmd = startup_model(&mut model, startup);
    let effects = drained(&mut model, cmd)
        .into_iter()
        .chain(roll_pending(&model.playlist))
        .collect();
    (model, effects)
}

pub(crate) fn startup_model(model: &mut Model, startup: Startup) -> Cmd {
    let keymap = Keymap::new(startup.keymap_overrides);
    let errors = startup
        .errors
        .into_iter()
        .chain(
            keymap
                .diagnostic()
                .map(|diagnostic| (ConfigName::Config, ConfigError::Parse(diagnostic))),
        )
        .collect();
    model.workspace.keymap = keymap;
    model.settings = Settings {
        audio_settings: startup.audio_settings,
        output_devices: Vec::new(),
        device_name: None,
        appearance_settings: startup.appearance_settings,
    };
    model.transport.volume = startup.volume;

    model.library = None;
    model.music_dir = startup.music_dir;
    model.playlist_source = startup.playlist_source;
    model.themes = Themes {
        names: startup.theme_names,
        theme_choice: startup.theme_choice,
    };
    model
        .playlist
        .relist(startup.playlist_tracks, startup.playlist_index);
    let browse = &mut model.workspace.browse;
    browse.cursor = browse.cursor.resize(model.playlist.tracks.len());
    model.playlist.play_order = match startup.shuffle {
        Shuffle::On => PlayOrder::ShufflePending,
        Shuffle::Off => PlayOrder::Linear,
    };

    let toasts = startup_toasts(model, errors);
    stopped_effects()
        .then(startup_cmd(model, DriverName::Audio))
        .then(startup_cmd(model, DriverName::Library))
        .then(startup_cmd(model, DriverName::Config))
        .then(toasts)
}

pub(crate) fn startup_cmd(model: &mut Model, driver_name: DriverName) -> Cmd {
    match driver_name {
        DriverName::Audio => Cmd::from_iter([
            Effect::Audio(AudioCmd::ListDevices),
            Effect::Audio(AudioCmd::SetDevice(
                model.settings.audio_settings.device.clone(),
            )),
            Effect::Audio(AudioCmd::SetCrossfade(
                model.settings.audio_settings.crossfade,
            )),
            Effect::Audio(AudioCmd::SetReplayGain(
                model.settings.audio_settings.replay_gain,
            )),
        ]),
        DriverName::Library => Cmd::from_iter([
            Effect::Library(LibraryCmd::Disk(DiskCmd::LoadFavorites)),
            Effect::Library(LibraryCmd::Scan {
                music_dir: model.music_dir.clone(),
                revision: model.revisions.issue_scan(),
                mode: ScanMode::Cached,
            }),
        ]),
        DriverName::Config => {
            Effect::Config(ConfigCmd::SelectTheme(model.themes.theme_choice.clone()))
                .into()
        }
        DriverName::Macos => Cmd::none(),
    }
}

fn startup_toasts(model: &mut Model, errors: Vec<(ConfigName, ConfigError)>) -> Cmd {
    let toast = errors.first().map(|(name, error)| trouble(name, error));
    model.workspace.config_errors.extend(errors);
    toast.map_or(Cmd::none(), |toast| {
        model.workspace.show(toast, &mut model.revisions)
    })
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
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            index::ViewIndex,
            keymap::{Action, KeyOverride, KeymapOverrides},
            model::Model,
            percent::Percent,
            playlist::{PlayOrder, PlaylistSource},
            settings::{AudioSettings, ReplayGain},
            sleep_presets::SleepPresets,
            startup::{Shuffle, Startup},
            theme::{ThemeChoice, ThemeName},
            track::{Track, TrackSource},
        },
        update::startup::startup_model,
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
            shuffle: Shuffle::On,
            audio_settings: AudioSettings {
                crossfade: Crossfade::clamped(Duration::from_secs(3)),
                replay_gain: ReplayGain::On,
                device: OutputDevice::Named(
                    DeviceName::new("Speakers".to_string()).unwrap(),
                ),
                sleep_presets: SleepPresets::from_minutes(&[15, 30]).unwrap(),
            },
            appearance_settings: crate::domain::appearance::AppearanceSettings::default(
            ),
            theme_choice: ThemeChoice::Named(ThemeName::from_static("dark")),
            volume: Percent::clamped(42),
            keymap_overrides: KeymapOverrides::default(),
            theme_names: vec![
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar"),
            ],
            errors: Vec::new(),
        }
    }

    #[test]
    fn startup_errors_raise_one_toast_with_the_first_error() {
        let mut model = Model::default();
        let broken_error = crate::domain::config::ConfigError::from(
            crate::domain::config::Diagnostic::from_error(&std::io::Error::other(
                "broken",
            )),
        );
        let unreadable_error = crate::domain::config::ConfigError::Read {
            name: crate::domain::config::ConfigName::Appearance,
            error: crate::domain::io_error::IoError::Other,
        };
        let startup = Startup {
            errors: vec![
                (
                    crate::domain::config::ConfigName::Theme(ThemeName::from_static(
                        "ghost",
                    )),
                    broken_error.clone(),
                ),
                (
                    crate::domain::config::ConfigName::Appearance,
                    unreadable_error.clone(),
                ),
            ],
            ..stock_startup()
        };

        drop(startup_model(&mut model, startup));

        let texts: Vec<_> = model
            .workspace
            .toasts
            .iter()
            .map(|toast| (toast.level, toast.text.clone()))
            .collect();
        assert_eq!(
            texts,
            [(
                crate::domain::toast::ToastLevel::Error,
                Some(broken_error.to_string())
            )]
        );
        assert_eq!(
            model.workspace.config_errors.replace(
                crate::domain::config::ConfigName::Appearance,
                unreadable_error
            ),
            Err(crate::update::machine::Unhandled)
        );
        assert_eq!(
            model.workspace.config_errors.replace(
                crate::domain::config::ConfigName::Theme(ThemeName::from_static(
                    "ghost"
                )),
                broken_error
            ),
            Err(crate::update::machine::Unhandled)
        );
    }

    #[test]
    fn startup_applies_the_keymap_overrides() {
        let mut model = Model::default();
        let keymap_overrides =
            KeymapOverrides::from([(Action::Next, KeyOverride::from("m"))]);
        let startup = Startup {
            keymap_overrides: keymap_overrides.clone(),
            ..stock_startup()
        };

        drop(startup_model(&mut model, startup));

        assert_eq!(model.workspace.keymap.overrides(), &keymap_overrides);
        assert!(model.workspace.toasts.is_empty());
    }

    #[test]
    fn startup_without_notices_raises_no_toast() {
        let mut model = Model::default();
        drop(startup_model(&mut model, stock_startup()));

        assert!(model.workspace.toasts.is_empty());
    }

    fn started_model() -> Model {
        let mut model = Model::default();
        drop(startup_model(&mut model, stock_startup()));
        model
    }

    #[test]
    fn startup_seeds_settings_transport_and_themes() {
        let model = started_model();

        insta::assert_debug_snapshot!((
            model.settings,
            model.transport.volume,
            model.themes,
        ));
    }

    #[test]
    fn startup_leaves_library_loading_and_seeds_the_playlist() {
        let model = started_model();

        assert!(model.library.is_none());
        assert_eq!(model.playlist.tracks.len(), 2);
        assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(0)));
    }

    #[test]
    fn startup_seeds_music_dir_and_requests_a_library_scan() {
        let mut model = Model::default();
        let cmd = startup_model(&mut model, stock_startup());

        assert_eq!(model.music_dir, PathBuf::from("/music"));
        let (effects, _messages) = cmd.into_parts();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Library(LibraryCmd::Scan { music_dir, .. }) if music_dir == Path::new("/music")
        )));
    }

    #[test]
    fn startup_seeds_shuffle_favorites_and_themes() {
        let model = started_model();

        assert!(model.settings.output_devices.is_empty());
        assert_eq!(model.playlist.play_order, PlayOrder::ShufflePending);
        assert!(
            !model
                .favorites
                .is_favorite(&TrackSource::Local("/music/a.flac".into()))
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
        let cmd = startup_model(&mut model, stock_startup());

        let (effects, _messages) = cmd.into_parts();
        insta::assert_debug_snapshot!(effects);
    }

    fn idempotence_fields(model: &Model) -> impl std::fmt::Debug + PartialEq {
        (
            model.settings.audio_settings.clone(),
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
        drop(startup_model(&mut first, stock_startup()));

        let mut second = Model::default();
        drop(startup_model(&mut second, stock_startup()));
        drop(startup_model(&mut second, stock_startup()));

        let projected = idempotence_fields(&second);
        assert_eq!(idempotence_fields(&first), projected);
        insta::assert_debug_snapshot!(projected);
    }
}
