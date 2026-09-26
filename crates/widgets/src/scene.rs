use std::{path::Path, time::Duration};

use config::{AppearanceFile, CoverStyle};
use kernel::{
    Moment,
    domain::{DeviceName, Model, Overlay, ThemeChoice},
    update::keymap::KeyBinding,
};
use raster::color_overrides;

use crate::{
    card::CardView,
    geometry::{CellAspect, CoverSizing, cover_sizing},
    key_hints::KeyHintsContent,
    overlay::{layer::OverlayContent, settings::SettingsView},
    playlist::{LibraryLoad, PlaylistPane, PlaylistView},
    screen::LayoutInputs,
    spectrum::Spectrum,
    theme::{ActiveTheme, ColorDepth, Theme},
    toast::ToastCard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelPath {
    Protocol,
    Halfblocks,
}

#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    pub model: &'a Model,
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    pub appearance: &'a AppearanceFile,
    pub bindings: &'a [KeyBinding],
    pub spectrum: &'a Spectrum,
    pub pixel_path: PixelPath,
    pub cell_aspect: CellAspect,
    pub clock: Duration,
    pub now_unix: u64,
    pub now: Moment,
    pub music_dir: &'a str,
    pub sleep_left: Option<Duration>,
}

impl<'a> Scene<'a> {
    #[must_use]
    pub fn active_theme(&self) -> ActiveTheme<'a> {
        ActiveTheme::new(self.theme, self.color_depth)
            .with_bars(color_overrides(&self.appearance.progress))
    }

    #[must_use]
    pub fn card_view(&self) -> CardView<'a> {
        let model = self.model;
        CardView {
            player: &model.player,
            speed: model.transport.speed,
            volume: model.transport.volume,
            spectrum: self.spectrum,
            repeat: model.playlist.repeat,
            play_order: &model.playlist.play_order,
            queue_length: model.queue.len(),
            displayed_track: model.displayed_track(),
            output: &model.transport.output,
            now: self.now,
        }
    }

    #[must_use]
    pub(crate) fn playlist_view(&self) -> PlaylistView<'a> {
        let model = self.model;
        PlaylistView {
            playlist: &model.playlist,
            queue: &model.queue,
            favorites: &model.favorites,
            browse_selected: model.workspace.browse.selected().get(),
            playing: model.playing_index(),
            library_loading: if model.library.is_loading() {
                LibraryLoad::Loading
            } else {
                LibraryLoad::Ready
            },
            scan: model.scan_status,
            sleep_left: self.sleep_left,
        }
    }

    #[must_use]
    pub(crate) fn settings_view(&self) -> SettingsView<'a> {
        let settings = &self.model.settings;
        SettingsView {
            crossfade: settings.crossfade,
            replaygain: settings.replaygain,
            theme: theme_display(&self.model.themes.selected),
            themes: &self.model.themes.names,
            sleep_presets: &settings.sleep_presets,
            music_dir: self.music_dir,
            output_device: settings.output_device.as_ref().map(DeviceName::as_str),
            output_devices: &settings.output_devices,
            appearance: self.appearance.options(),
            custom_rows: &self.model.custom_rows,
        }
    }

    #[must_use]
    pub(crate) fn key_hints(&self) -> KeyHintsContent<'a> {
        KeyHintsContent::new(self.model.workspace.overlay.as_ref(), self.bindings)
    }

    #[must_use]
    pub fn cover_style(&self) -> CoverStyle {
        painted_cover_style(self.appearance.cover.style, self.pixel_path)
    }

    #[must_use]
    pub fn cover_sizing(&self) -> CoverSizing {
        cover_sizing(self.cover_style(), self.appearance.cover.text_cells)
    }

    #[must_use]
    pub(crate) fn playlist_pane(&self) -> Option<PlaylistPane<'a>> {
        let taken = matches!(
            self.model.workspace.overlay,
            Some(Overlay::Search(_) | Overlay::History(_))
        );
        if taken {
            return None;
        }
        Some(PlaylistPane {
            view: self.playlist_view(),
            theme: self.active_theme(),
        })
    }

    #[must_use]
    pub(crate) fn overlay_content(&self) -> OverlayContent<'a> {
        let model = self.model;
        OverlayContent {
            workspace: &model.workspace,
            theme: self.active_theme(),
            tracks: &model.playlist.tracks,
            history: &model.history.view,
            settings_view: self.settings_view(),
            bindings: self.bindings,
            now_unix: self.now_unix,
        }
    }

    #[must_use]
    pub(crate) fn toast_card(&self) -> Option<ToastCard<'a>> {
        let toast = self.model.workspace.toast.as_ref()?;
        Some(ToastCard {
            toast,
            theme: self.active_theme(),
        })
    }

    #[must_use]
    pub fn layout_inputs(&self) -> LayoutInputs<'a> {
        LayoutInputs {
            breakpoints: &self.appearance.layout,
            window: self.appearance.window,
            cell_aspect: self.cell_aspect,
            cover_sizing: self.cover_sizing(),
            cover_style: self.cover_style(),
            playlist: self.playlist_pane(),
            overlay: self.overlay_content(),
            toast: self.toast_card(),
        }
    }
}

fn theme_display(choice: &ThemeChoice) -> &str {
    match choice {
        ThemeChoice::Auto => "auto",
        ThemeChoice::Named(name) => name.as_str(),
    }
}

fn painted_cover_style(style: CoverStyle, detected: PixelPath) -> CoverStyle {
    match (style, detected) {
        (CoverStyle::Vinyl | CoverStyle::Plain, PixelPath::Halfblocks)
        | (CoverStyle::Off, PixelPath::Protocol | PixelPath::Halfblocks) => {
            CoverStyle::Off
        }
        (CoverStyle::Vinyl | CoverStyle::Plain, PixelPath::Protocol)
        | (CoverStyle::Milkdrop, PixelPath::Protocol | PixelPath::Halfblocks) => style,
    }
}

#[must_use]
pub fn abbreviate_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::{sync::Arc, time::Duration};

    use config::{Appearance, AppearanceFile};
    use kernel::{
        Moment,
        domain::{
            AudioFormat,
            Crossfade,
            CustomSetting,
            KeymapOverrides,
            Model,
            Replaygain,
            Tags,
            Track,
        },
        playlist::Playlist,
        update::keymap::Bindings,
    };
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, widgets::Widget};

    use crate::{
        geometry::CellAspect,
        overlay::settings::SettingsView,
        scene::{PixelPath, Scene},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        theme::{ColorDepth, Theme},
    };

    pub(crate) fn noir() -> Theme {
        let file =
            config::parse_theme(include_str!("../../../themes/noir.toml"), "noir")
                .unwrap();
        Theme::from(file)
    }

    pub(crate) fn painted<W>(widget: &W, width: u16, height: u16) -> String
    where
        for<'a> &'a W: Widget,
    {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(widget, frame.area()))
            .unwrap();
        format!("{}", terminal.backend())
    }

    pub(crate) fn painted_buffer<W>(widget: &W, width: u16, height: u16) -> Buffer
    where
        for<'a> &'a W: Widget,
    {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(widget, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    pub(crate) fn find_text(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
        let wanted: Vec<char> = needle.chars().collect();
        let width = wanted.len();
        if width == 0 {
            return None;
        }
        for y in 0..buffer.area.height {
            let symbols: Vec<&str> = (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            if symbols.len() < width {
                continue;
            }
            for start in 0..=symbols.len() - width {
                let matched = wanted.iter().enumerate().all(|(offset, glyph)| {
                    symbols
                        .get(start + offset)
                        .is_some_and(|symbol| *symbol == glyph.to_string())
                });
                if matched {
                    return u16::try_from(start).ok().map(|x| (x, y));
                }
            }
        }
        None
    }

    pub(crate) fn custom_rows() -> Vec<CustomSetting> {
        config::custom_rows(&AppearanceFile::default())
    }

    pub(crate) fn settings_values(custom_rows: &[CustomSetting]) -> SettingsView<'_> {
        SettingsView {
            crossfade: Crossfade::default(),
            replaygain: Replaygain::On,
            theme: "noir",
            themes: &[],
            sleep_presets: &[],
            music_dir: "/home/user/Music",
            output_device: None,
            output_devices: &[],
            appearance: Appearance::default(),
            custom_rows,
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
        pub(crate) appearance: AppearanceFile,
        pub(crate) bindings: Bindings,
        pub(crate) spectrum: Spectrum,
    }

    impl SceneSources {
        pub(crate) fn new(model: Model) -> Self {
            Self {
                model,
                theme: noir(),
                appearance: AppearanceFile::default(),
                bindings: Bindings::new(&KeymapOverrides::default()),
                spectrum: [0.0; SPECTRUM_BANDS],
            }
        }

        pub(crate) fn scene(&self) -> Scene<'_> {
            Scene {
                model: &self.model,
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                appearance: &self.appearance,
                bindings: self.bindings.as_slice(),
                spectrum: &self.spectrum,
                pixel_path: PixelPath::Halfblocks,
                cell_aspect: CellAspect::default(),
                clock: Duration::ZERO,
                now_unix: 0,
                now: Moment::default(),
                music_dir: "/home/user/Music",
                sleep_left: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use config::CoverStyle;
    use rstest::rstest;

    use crate::scene::{PixelPath, abbreviate_home, painted_cover_style};

    #[rstest]
    #[case::vinyl_with_graphics(
        CoverStyle::Vinyl,
        PixelPath::Protocol,
        CoverStyle::Vinyl
    )]
    #[case::plain_with_graphics(
        CoverStyle::Plain,
        PixelPath::Protocol,
        CoverStyle::Plain
    )]
    #[case::vinyl_without_graphics(
        CoverStyle::Vinyl,
        PixelPath::Halfblocks,
        CoverStyle::Off
    )]
    #[case::plain_without_graphics(
        CoverStyle::Plain,
        PixelPath::Halfblocks,
        CoverStyle::Off
    )]
    #[case::off_with_graphics(CoverStyle::Off, PixelPath::Protocol, CoverStyle::Off)]
    #[case::off_without_graphics(
        CoverStyle::Off,
        PixelPath::Halfblocks,
        CoverStyle::Off
    )]
    #[case::milkdrop_with_graphics(
        CoverStyle::Milkdrop,
        PixelPath::Protocol,
        CoverStyle::Milkdrop
    )]
    #[case::milkdrop_without_graphics(
        CoverStyle::Milkdrop,
        PixelPath::Halfblocks,
        CoverStyle::Milkdrop
    )]
    fn the_painted_cover_style_reads_the_style_and_the_terminal(
        #[case] style: CoverStyle,
        #[case] detected: PixelPath,
        #[case] expected: CoverStyle,
    ) {
        assert_eq!(painted_cover_style(style, detected), expected);
    }

    #[rstest]
    #[case::under_home(
        "/Users/test/Desktop/apple-music",
        "/Users/test",
        "~/Desktop/apple-music"
    )]
    #[case::equal_to_home("/Users/test", "/Users/test", "~")]
    #[case::outside_home("/mnt/music", "/Users/test", "/mnt/music")]
    fn a_path_under_home_starts_with_a_tilde(
        #[case] path: &str,
        #[case] home: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(abbreviate_home(Path::new(path), Path::new(home)), expected);
    }
}
