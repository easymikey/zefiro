use std::{path::Path, time::Duration};

use config::{AppearanceFile, CoverStyle, ProgressStyle};
use kernel::{
    Moment,
    domain::{DeviceName, Model, Overlay, ThemeChoice},
    update::keymap::KeyBinding,
};
use raster::color_overrides;

use crate::{
    card::{CardMetrics, CardView, compact_progress_bar_width},
    geometry::{CellAspect, CoverSizing, cover_sizing},
    key_hints::KeyHintsContent,
    overlay::{layer::OverlayContent, settings::SettingsView},
    playlist::{LibraryLoad, PlaylistPane, PlaylistView},
    primitive::bar::hud_progress_bar_width,
    redraw::{OnScreen, Presence},
    screen::{Breakpoint, FrameLayout, LayoutInputs, minimal_progress_bar_width},
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

    #[must_use]
    pub fn on_screen(&self, layout: &FrameLayout) -> OnScreen {
        OnScreen {
            progress_bar: self.progress_bar_width(layout),
            clock: if self.card_shown(layout) {
                Presence::Shown
            } else {
                Presence::Hidden
            },
            sleep_label: if layout.playlist.is_some() && self.sleep_left.is_some() {
                Presence::Shown
            } else {
                Presence::Hidden
            },
            spectrum: if self.spectrum_shown(layout) {
                Presence::Shown
            } else {
                Presence::Hidden
            },
        }
    }

    fn card_shown(&self, layout: &FrameLayout) -> bool {
        match layout.breakpoint {
            Breakpoint::Full => layout.card.is_some(),
            Breakpoint::Compact | Breakpoint::Minimal => true,
            Breakpoint::TooSmall => false,
        }
    }

    fn spectrum_shown(&self, layout: &FrameLayout) -> bool {
        let card_spectrum = layout
            .card
            .is_some_and(|metrics| !metrics.spectrum_row.is_empty());
        let milkdrop_spectrum =
            self.cover_style() == CoverStyle::Milkdrop && layout.cover.is_some();
        card_spectrum || milkdrop_spectrum
    }

    fn progress_bar_width(&self, layout: &FrameLayout) -> Option<u16> {
        match layout.breakpoint {
            Breakpoint::Full => layout
                .card
                .map(|metrics| self.full_progress_bar_width(&metrics)),
            Breakpoint::Compact => Some(compact_progress_bar_width(layout.header)),
            Breakpoint::Minimal => Some(minimal_progress_bar_width(
                self.card_view(),
                self.appearance.options().speed_chip,
                layout.screen.width,
            )),
            Breakpoint::TooSmall => None,
        }
    }

    fn full_progress_bar_width(&self, metrics: &CardMetrics) -> u16 {
        let row_width = metrics.progress_row.width;
        match self.appearance.options().progress_remaining {
            ProgressStyle::Remaining => {
                let view = self.card_view();
                let duration = view
                    .displayed_track
                    .and_then(|track| track.duration())
                    .unwrap_or(Duration::ZERO);
                let remaining =
                    duration.saturating_sub(view.player.position_at(view.now));
                hud_progress_bar_width(row_width, remaining)
            }
            ProgressStyle::Elapsed => row_width,
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
                now: Moment::default(),
                music_dir: "/home/user/Music",
                sleep_left: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use config::{CoverStyle, ProgressStyle};
    use ratatui::layout::{Rect, Size};
    use rstest::rstest;

    use crate::{
        card::compact_progress_bar_width,
        primitive::bar::hud_progress_bar_width,
        redraw::Presence,
        scene::{
            PixelPath,
            abbreviate_home,
            fixtures::{SceneSources, model_with_tracks},
            painted_cover_style,
        },
        screen::{Breakpoint, FrameLayout},
    };

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

    #[rstest]
    #[case::full_with_chip(
        Size::new(80, 24),
        (48, 16),
        Some(ProgressStyle::Remaining)
    )]
    #[case::full_without_chip(
        Size::new(80, 24),
        (48, 16),
        Some(ProgressStyle::Elapsed)
    )]
    #[case::compact(Size::new(80, 18), (48, 16), None)]
    #[case::minimal(Size::new(20, 5), (10, 3), None)]
    #[case::overlay_only(Size::new(40, 10), (48, 16), None)]
    fn on_screen_rows(
        #[case] size: Size,
        #[case] minimums: (u16, u16),
        #[case] style: Option<ProgressStyle>,
    ) {
        let (min_columns, min_rows) = minimums;
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.appearance.layout.min_columns = min_columns;
        sources.appearance.layout.min_rows = min_rows;
        if let Some(style) = style {
            sources.appearance.progress.remaining = style;
        }
        let scene = sources.scene();
        let layout = FrameLayout::new(
            &scene.layout_inputs(),
            Rect::new(0, 0, size.width, size.height),
        );
        let on_screen = scene.on_screen(&layout);

        match layout.breakpoint {
            Breakpoint::Full => {
                let metrics = layout.card.unwrap();
                let row_width = metrics.progress_row.width;
                let expected = match style.unwrap() {
                    ProgressStyle::Remaining => {
                        let view = scene.card_view();
                        let duration = view
                            .displayed_track
                            .and_then(|track| track.duration())
                            .unwrap_or(Duration::ZERO);
                        let remaining =
                            duration.saturating_sub(view.player.position_at(view.now));
                        hud_progress_bar_width(row_width, remaining)
                    }
                    ProgressStyle::Elapsed => row_width,
                };
                assert_eq!(on_screen.progress_bar, Some(expected));
                assert_eq!(on_screen.clock, Presence::Shown);
            }
            Breakpoint::Compact => {
                assert_eq!(
                    on_screen.progress_bar,
                    Some(compact_progress_bar_width(layout.header))
                );
                assert_eq!(on_screen.clock, Presence::Shown);
            }
            Breakpoint::Minimal => {
                assert!(on_screen.progress_bar.is_some());
                assert_eq!(on_screen.clock, Presence::Shown);
            }
            Breakpoint::TooSmall => {
                assert_eq!(on_screen.progress_bar, None);
                assert_eq!(on_screen.clock, Presence::Hidden);
            }
        }
    }
}
