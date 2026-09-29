#![cfg(test)]

use std::{sync::Arc, time::Duration};

use config::AppearanceFile;
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
    },
    update::keymap::{Bindings, KeyBinding},
};
use raster::VinylColors;
use terminal::{CoverKey, CoverLook, CoverMoment, CoverPlacement, CoverSources};
use widgets::{
    CellAspect,
    ColorDepth,
    MilkdropColors,
    PixelPath,
    Playing,
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
            now: Moment::default(),
            music_dir: "/home/user/Music",
            sleep_left: None,
        }
    }

    pub(crate) fn sources(&self, placement: CoverPlacement) -> CoverSources<'_> {
        self.sources_at(placement, Duration::ZERO)
    }

    pub(crate) fn sources_at(
        &self,
        placement: CoverPlacement,
        clock: Duration,
    ) -> CoverSources<'_> {
        let scene = self.scene_at(clock);
        CoverSources {
            key: CoverKey {
                config_generation: scene.model.config_generation,
                theme_generation: scene.model.theme_generation,
            },
            look: CoverLook {
                style: scene.cover_style(),
                animations: scene.appearance.window.animations,
                vinyl: VinylColors::from(scene.theme),
                milkdrop: MilkdropColors::from_theme(&scene.active_theme()),
            },
            moment: CoverMoment {
                clock: scene.clock,
                playing: if scene.model.player.is_playing() {
                    Playing::Yes
                } else {
                    Playing::No
                },
                track: scene.model.player.current().map(|track| track.path()),
                bands: scene.spectrum,
            },
            placement,
        }
    }
}
