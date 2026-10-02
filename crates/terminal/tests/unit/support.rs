#![cfg(test)]

use std::{sync::Arc, time::Duration};

use kernel::{
    Moment,
    domain::{
        AudioFormat,
        KeymapOverrides,
        Model,
        Player,
        Playhead,
        Preload,
        Speed,
        Tags,
        Track,
        appearance::Look,
    },
    update::keymap::{Bindings, KeyBinding},
};
use widgets::{
    ColorDepth,
    Colors,
    DEFAULT_CELL_ASPECT,
    PixelPath,
    SPECTRUM_BANDS,
    Scene,
    ScenePresentation,
    Spectrum,
    Theme,
    ThemeSeed,
};

pub(crate) fn noir_theme() -> Theme {
    let file =
        config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
            .unwrap();
    let c = file.colors;
    let palette = ThemeSeed {
        background: c.background,
        foreground: c.foreground,
        bright_foreground: c.bright_foreground,
        accent: c.accent,
        green: c.green,
        yellow: c.yellow,
        red: c.red,
        window_background: c.window_background,
    };
    Theme {
        name: file.name,
        colors: Colors::derive(&palette),
        scanning_label: file.scanning_label,
    }
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
            head: Playhead::anchored(
                Duration::from_secs(position_secs),
                Moment::default(),
                Speed::default(),
            ),
            preload: Preload::None,
        },
        ..Model::default()
    }
}

pub(crate) struct Scenery {
    pub(crate) model: Model,
    pub(crate) theme: Theme,
    pub(crate) bindings: Vec<KeyBinding>,
    pub(crate) spectrum: Spectrum,
}

impl Scenery {
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            theme: noir_theme(),
            bindings: bindings(),
            spectrum: [0.0; SPECTRUM_BANDS],
        }
    }

    pub(crate) fn look_mut(&mut self) -> &mut Look {
        &mut self.model.settings.look
    }

    pub(crate) fn scene_at(&self, clock: Duration) -> Scene<'_> {
        Scene::from_model(
            &self.model,
            ScenePresentation {
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                bindings: &self.bindings,
                spectrum: &self.spectrum,
                pixel_path: PixelPath::Protocol,
                cell_aspect: DEFAULT_CELL_ASPECT,
                clock,
                now: Moment::default(),
                music_dir: "/home/user/Music",
                sleep_left: None,
            },
        )
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        self.scene_at(Duration::ZERO)
    }
}
