use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::domain::{
    appearance::{Appearance, Rgb},
    model::Model,
    player::Player,
    playhead::Playhead,
    speed::Speed,
    theme::ThemeName,
    time::Moment,
    track::{AudioFormat, Tags, Track},
};
use widgets::{
    geometry::DEFAULT_CELL_ASPECT,
    key_hints::KeyHintChords,
    scene::{PixelPath, Scene, ScenePresentation},
    spectrum::{SPECTRUM_BANDS, Spectrum},
    theme::{
        Theme,
        colors::{Colors, ThemeBase},
        rgb::ColorDepth,
    },
};

pub(crate) fn noir_theme() -> Theme {
    Theme {
        name: ThemeName::from_static("noir"),
        colors: Colors::derive(&ThemeBase {
            background: Rgb([0x0a, 0x0c, 0x10]),
            muted_foreground: Rgb([0x6b, 0x72, 0x80]),
            foreground: Rgb([0xd8, 0xdd, 0xe6]),
            accent: Rgb([0xd9, 0x77, 0x57]),
            green: Rgb([0x6b, 0x72, 0x80]),
            yellow: Rgb([0xd9, 0x77, 0x57]),
            red: Rgb([0xf2, 0xa8, 0x78]),
            window_background: None,
        }),
        scanning_label: "scanning…".to_owned(),
    }
}

pub(crate) fn track(title: &str, duration_secs: u64) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(format!("/music/{title}.mp3"))
            .duration(Duration::from_secs(duration_secs))
            .tags(Tags {
                title: Some(title.to_string()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
}

pub(crate) fn playing_model(
    title: &str,
    duration_secs: u64,
    position_secs: u64,
) -> Model {
    Model {
        player: Player::Playing {
            track: track(title, duration_secs),
            playhead: Playhead::anchored(
                Duration::from_secs(position_secs),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        ..Model::default()
    }
}

pub(crate) struct Scenery {
    pub(crate) model: Model,
    pub(crate) theme: Theme,
    pub(crate) spectrum: Spectrum,
    pub(crate) appearance: Appearance,
    pub(crate) key_hint_chords: KeyHintChords,
}

impl Scenery {
    pub(crate) fn new(mut model: Model) -> Self {
        model.music_dir = PathBuf::from("/home/user/Music");
        let key_hint_chords =
            KeyHintChords::from_bindings(model.workspace.keymap.bindings());
        Self {
            model,
            theme: noir_theme(),
            spectrum: [0.0; SPECTRUM_BANDS],
            appearance: Appearance::default(),
            key_hint_chords,
        }
    }

    pub(crate) fn scene_at(&self, clock: Duration) -> Scene<'_> {
        Scene::from_model(
            &self.model,
            ScenePresentation {
                appearance: &self.appearance,
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                spectrum: &self.spectrum,
                pixel_path: PixelPath::Protocol,
                cell_aspect: DEFAULT_CELL_ASPECT,
                clock,
                now: Moment::default(),
                home: None,
                key_hint_chords: &self.key_hint_chords,
            },
        )
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        self.scene_at(Duration::ZERO)
    }
}
