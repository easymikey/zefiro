use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::domain::{
    model::Model,
    playlist::Playlist,
    time::Moment,
    track::{AudioFormat, Tags, Track},
};
use ratatui::{Frame, Terminal, backend::TestBackend};
use widgets::{
    appearance::Appearance,
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
    let file = config::theme_file::parse_theme(
        include_str!("../../../../themes/noir.toml"),
        "noir",
    )
    .unwrap();
    theme_of(file)
}

pub(crate) fn theme_of(file: config::theme_file::TomlTheme) -> Theme {
    let c = file.colors;
    let palette = ThemeBase {
        background: c.background,
        muted_foreground: c.muted_foreground,
        foreground: c.foreground,
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

pub(crate) fn track(title: &str) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(format!("/music/{title}.mp3"))
            .duration(Duration::from_secs(245))
            .tags(Tags {
                title: Some(title.to_string()),
                artist: Some("Test Artist".to_string()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
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
                pixel_path: PixelPath::Halfblocks,
                cell_aspect: DEFAULT_CELL_ASPECT,
                clock: Duration::ZERO,
                now: Moment::default(),
                home: None,
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
