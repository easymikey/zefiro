use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::domain::{
    appearance::{Appearance, Rgb},
    model::Model,
    playlist::Playlist,
    theme::ThemeName,
    time::Moment,
    track::{AudioFormat, Tags, Track, TrackParts},
};
use ratatui::{Frame, Terminal, backend::TestBackend};
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

pub(crate) fn noir() -> Theme {
    stock_theme(
        ThemeName::from_static("noir"),
        &ThemeBase {
            background: Rgb([0x0a, 0x0c, 0x10]),
            muted_foreground: Rgb([0x6b, 0x72, 0x80]),
            foreground: Rgb([0xd8, 0xdd, 0xe6]),
            accent: Rgb([0xd9, 0x77, 0x57]),
            green: Rgb([0x6b, 0x72, 0x80]),
            yellow: Rgb([0xd9, 0x77, 0x57]),
            red: Rgb([0xf2, 0xa8, 0x78]),
            window_background: None,
        },
    )
}

pub(crate) fn stock_theme(name: ThemeName, theme_base: &ThemeBase) -> Theme {
    Theme {
        name,
        colors: Colors::from_theme_base(theme_base),
        scanning_label: "scanning…".to_owned(),
    }
}

pub(crate) fn track(title: &str) -> Arc<Track> {
    Arc::new(Track::new(TrackParts {
        path: format!("/music/{title}.mp3").into(),
        duration: Duration::from_secs(245),
        tags: Tags {
            title: Some(title.to_string()),
            artist: Some("Test Artist".to_string()),
            ..Tags::default()
        },
        audio_format: AudioFormat::default(),
    }))
}

pub(crate) fn model_with_tracks(count: usize) -> Model {
    Model {
        playlist: Playlist {
            tracks: (0..count)
                .map(|index| track(&format!("song{index:02}")))
                .collect(),
            ..Playlist::default()
        },
        ..Model::default()
    }
}

#[derive(Debug)]
pub(crate) struct SceneSources {
    pub(crate) model: Model,
    pub(crate) theme: Theme,
    pub(crate) spectrum: Spectrum,
    pub(crate) appearance: Appearance,
    pub(crate) key_hint_chords: KeyHintChords,
    pub(crate) pixel_path: PixelPath,
}

impl SceneSources {
    pub(crate) fn new(mut model: Model) -> Self {
        model.music_dir = PathBuf::from("/home/user/Music");
        let key_hint_chords =
            KeyHintChords::from_bindings(model.workspace.keymap.bindings());
        Self {
            model,
            theme: noir(),
            spectrum: [0.0; SPECTRUM_BANDS],
            appearance: Appearance::default(),
            key_hint_chords,
            pixel_path: PixelPath::Halfblocks,
        }
    }

    pub(crate) fn appearance_mut(&mut self) -> &mut Appearance {
        &mut self.appearance
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        Scene::from_model(
            &self.model,
            ScenePresentation {
                appearance: &self.appearance,
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                spectrum: &self.spectrum,
                pixel_path: self.pixel_path,
                cell_aspect: DEFAULT_CELL_ASPECT,
                since_first_paint: Duration::ZERO,
                now: Moment::default(),
                home_dir: None,
                key_hint_chords: &self.key_hint_chords,
            },
        )
    }
}

pub(crate) fn rendered(
    width: u16,
    height: u16,
    paint: impl FnOnce(&mut Frame<'_>),
) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(paint).unwrap();
    terminal.backend().clone()
}
