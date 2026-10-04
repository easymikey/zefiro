use std::{sync::Arc, time::Duration};

use kernel::{
    Moment,
    domain::{
        AudioFormat,
        KeymapOverrides,
        Model,
        Tags,
        Track,
        appearance::Appearance,
    },
    playlist::Playlist,
    update::keymap::{Bindings, KeyBinding},
};
use ratatui::{Frame, Terminal, backend::TestBackend};
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

pub(crate) fn noir() -> Theme {
    let file =
        config::parse_theme(include_str!("../../../themes/noir.toml"), "noir").unwrap();
    theme_of(file)
}

pub(crate) fn theme_of(file: config::ThemeFile) -> Theme {
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
    pub(crate) bindings: Vec<KeyBinding>,
    pub(crate) spectrum: Spectrum,
}

impl SceneSources {
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            theme: noir(),
            bindings: bindings(),
            spectrum: [0.0; SPECTRUM_BANDS],
        }
    }

    pub(crate) fn appearance_mut(&mut self) -> &mut Appearance {
        &mut self.model.settings.appearance
    }

    pub(crate) fn scene(&self) -> Scene<'_> {
        Scene::from_model(
            &self.model,
            ScenePresentation {
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                bindings: &self.bindings,
                spectrum: &self.spectrum,
                pixel_path: PixelPath::Halfblocks,
                cell_aspect: DEFAULT_CELL_ASPECT,
                clock: Duration::ZERO,
                now: Moment::default(),
                music_dir: "/home/user/Music",
                sleep_left: None,
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
