use std::{path::Path, time::Duration};

use config::{AppearanceFile, CoverStyle, ProgressTime};
use kernel::{
    Moment,
    domain::{DeviceName, Model, Overlay, ThemeChoice},
    update::keymap::KeyBinding,
};

use crate::{
    card::{CardMetrics, CardView, compact_progress_bar_width},
    geometry::{CoverSizing, cover_sizing},
    key_hints::KeyHintsContent,
    overlay::{layer::OverlayContent, settings::SettingsView},
    playlist::{LibraryLoad, PlaylistPane, PlaylistView},
    primitive::bar::hud_progress_bar_width,
    repaint::{OnScreen, Presence},
    screen::{Breakpoint, FrameLayout, FrameLayoutParts, minimal_progress_bar_width},
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
    pub cell_aspect: f32,
    pub clock: Duration,
    pub now: Moment,
    pub music_dir: &'a str,
    pub sleep_left: Option<Duration>,
}

impl<'a> Scene<'a> {
    #[must_use]
    pub fn active_theme(&self) -> ActiveTheme<'a> {
        ActiveTheme::new(self.theme, self.color_depth)
            .with_progress(&self.appearance.progress)
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
            library_loading: if model.library.is_none() {
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
        let audio = &settings.audio;
        SettingsView {
            crossfade: audio.crossfade,
            replaygain: audio.replaygain,
            theme: theme_label(&self.model.themes.selected),
            themes: &self.model.themes.names,
            sleep_presets: audio.sleep_presets.as_slice(),
            music_dir: self.music_dir,
            output_device: audio.device.named().map(DeviceName::as_str),
            output_devices: &settings.output_devices,
            appearance: self.appearance.appearance(),
            custom_settings: &self.model.custom_settings,
        }
    }

    #[must_use]
    pub(crate) fn key_hints(&self) -> KeyHintsContent<'a> {
        match self.model.workspace.overlay {
            Some(Overlay::Settings { .. }) => KeyHintsContent::settings(self.bindings),
            _ => KeyHintsContent::keys(self.bindings),
        }
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
        OverlayContent {
            model: self.model,
            theme: self.active_theme(),
            settings_view: self.settings_view(),
            bindings: self.bindings,
            now: self.now,
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
    pub fn layout_parts(&self) -> FrameLayoutParts<'a> {
        FrameLayoutParts {
            layout: &self.appearance.layout,
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
                self.appearance.appearance().speed_chip,
                layout.screen.width,
            )),
            Breakpoint::TooSmall => None,
        }
    }

    fn full_progress_bar_width(&self, metrics: &CardMetrics) -> u16 {
        let row_width = metrics.progress_row.width;
        match self.appearance.appearance().progress_time {
            ProgressTime::Remaining => {
                hud_progress_bar_width(row_width, self.card_view().remaining())
            }
            ProgressTime::Elapsed => row_width,
        }
    }
}

fn theme_label(choice: &ThemeChoice) -> &str {
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
mod tests {
    use std::path::Path;

    use config::{CoverStyle, ProgressTime};
    use ratatui::layout::{Rect, Size};
    use rstest::rstest;

    use crate::{
        card::compact_progress_bar_width,
        primitive::bar::hud_progress_bar_width,
        repaint::Presence,
        scene::{PixelPath, abbreviate_home, painted_cover_style},
        screen::{Breakpoint, FrameLayout},
        test_support::{SceneSources, model_with_tracks},
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
        Some(ProgressTime::Remaining)
    )]
    #[case::full_without_chip(
        Size::new(80, 24),
        (48, 16),
        Some(ProgressTime::Elapsed)
    )]
    #[case::compact(Size::new(80, 18), (48, 16), None)]
    #[case::minimal(Size::new(20, 5), (10, 3), None)]
    #[case::overlay_only(Size::new(40, 10), (48, 16), None)]
    fn the_breakpoint_picks_the_screen_for_the_size_and_minimums(
        #[case] size: Size,
        #[case] minimums: (u16, u16),
        #[case] style: Option<ProgressTime>,
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
            &scene.layout_parts(),
            Rect::new(0, 0, size.width, size.height),
        );
        let on_screen = scene.on_screen(&layout);

        match layout.breakpoint {
            Breakpoint::Full => {
                let metrics = layout.card.unwrap();
                let row_width = metrics.progress_row.width;
                let expected = match style.unwrap() {
                    ProgressTime::Remaining => {
                        hud_progress_bar_width(row_width, scene.card_view().remaining())
                    }
                    ProgressTime::Elapsed => row_width,
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
