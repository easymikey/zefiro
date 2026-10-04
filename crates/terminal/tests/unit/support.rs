#![cfg(test)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::domain::{
    model::Model,
    player::{Player, Preload},
    playhead::Playhead,
    speed::Speed,
    time::Moment,
    track::{AudioFormat, Tags, Track},
};
use widgets::{
    appearance::Appearance,
    geometry::DEFAULT_CELL_ASPECT,
    scene::{PixelPath, Scene, ScenePresentation},
    spectrum::{SPECTRUM_BANDS, Spectrum},
    theme::{
        Theme,
        colors::{Colors, ThemeBase},
        rgb::ColorDepth,
    },
};

pub(crate) fn noir_theme() -> Theme {
    let file = config::theme_file::parse_theme(
        include_str!("../../../../themes/noir.toml"),
        "noir",
    )
    .unwrap();
    let c = file.colors;
    let palette = ThemeBase {
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
    pub(crate) spectrum: Spectrum,
    pub(crate) appearance: Appearance,
}

impl Scenery {
    pub(crate) fn new(mut model: Model) -> Self {
        model.music_dir = PathBuf::from("/home/user/Music");
        Self {
            model,
            theme: noir_theme(),
            spectrum: [0.0; SPECTRUM_BANDS],
            appearance: Appearance::default(),
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
                sleep_left: None,
            },
        )
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        self.scene_at(Duration::ZERO)
    }
}
