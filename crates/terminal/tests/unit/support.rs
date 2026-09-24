#![cfg(test)]

use std::{sync::Arc, time::Duration};

use config::AppearanceFile;
use kernel::{
    domain::{AudioFormat, KeymapOverrides, Model, Player, Preload, Tags, Track},
    update::keymap::{Bindings, KeyBinding},
};
use widgets::{
    CellAspect,
    ColorDepth,
    PixelPath,
    SPECTRUM_BANDS,
    Scene,
    Spectrum,
    Theme,
};

pub(crate) fn noir_theme() -> Theme {
    let file =
        config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
            .unwrap();
    Theme::from(file)
}

pub(crate) fn bindings() -> Vec<KeyBinding> {
    Bindings::new(&KeymapOverrides::default())
        .as_slice()
        .to_vec()
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
            at: Duration::from_secs(position_secs),
            preload: Preload::None,
        },
        ..Model::default()
    }
}

pub(crate) struct Scenery {
    pub(crate) model: Model,
    pub(crate) theme: Theme,
    pub(crate) appearance: AppearanceFile,
    pub(crate) bindings: Vec<KeyBinding>,
    pub(crate) spectrum: Spectrum,
}

impl Scenery {
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            theme: noir_theme(),
            appearance: AppearanceFile::default(),
            bindings: bindings(),
            spectrum: [0.0; SPECTRUM_BANDS],
        }
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        self.scene_at(Duration::ZERO)
    }

    pub(crate) fn scene_at(&self, clock: Duration) -> Scene<'_> {
        Scene {
            model: &self.model,
            theme: &self.theme,
            color_depth: ColorDepth::TrueColor,
            appearance: &self.appearance,
            bindings: &self.bindings,
            spectrum: &self.spectrum,
            pixel_path: PixelPath::Protocol,
            cell_aspect: CellAspect::default(),
            clock,
            now_unix: 0,
            music_dir: "/home/user/Music",
            sleep_left: None,
        }
    }
}
