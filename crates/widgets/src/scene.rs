use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    Moment,
    domain::{
        AppearanceSetting,
        Favorites,
        HistoryEntry,
        Model,
        Overlay,
        Player,
        Revisions,
        ScanStatus,
        Settings,
        ThemeChoice,
        Themes,
        Toast,
        Track,
        TrackRef,
        Transport,
        ViewIndex,
        appearance::{Appearance, CoverMode, ProgressTime},
        geometry::Cells,
        playlist::Playlist,
    },
    update::keymap::KeyBinding,
};

use crate::{
    card::{CardMetrics, CardView, compact_progress_bar_width},
    geometry::{CoverSizing, cover_sizing},
    key_hints::KeyHintsView,
    playlist::LibraryLoad,
    primitive::bar::hud_progress_bar_width,
    repaint::{OnScreen, Presence},
    screen::{Breakpoint, FrameLayout, minimal_progress_bar_width},
    spectrum::Spectrum,
    theme::{ActiveTheme, ColorDepth, Theme},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelPath {
    Protocol,
    Halfblocks,
}

#[derive(Debug, Clone, Copy)]
pub struct ScenePresentation<'a> {
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    pub spectrum: &'a Spectrum,
    pub pixel_path: PixelPath,
    pub cell_aspect: f32,
    pub clock: Duration,
    pub now: Moment,
    pub home: Option<&'a Path>,
    pub sleep_left: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    pub player: &'a Player,
    pub transport: &'a Transport,
    pub playlist: &'a Playlist,
    pub queue: &'a [TrackRef],
    pub favorites: &'a Favorites,
    pub themes: &'a Themes,
    pub appearance_settings: &'a [AppearanceSetting],
    pub settings: &'a Settings,
    pub revisions: &'a Revisions,
    pub overlay: Option<&'a Overlay>,
    pub history: &'a [HistoryEntry],
    pub toasts: &'a [Toast],
    pub browse_selected: ViewIndex,
    pub playing: Option<ViewIndex>,
    pub displayed_track: Option<&'a Arc<Track>>,
    pub library_loading: LibraryLoad,
    pub scan: ScanStatus,
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    pub bindings: &'a [KeyBinding],
    pub spectrum: &'a Spectrum,
    pub pixel_path: PixelPath,
    pub cell_aspect: f32,
    pub clock: Duration,
    pub now: Moment,
    pub music_dir: &'a Path,
    pub home: Option<&'a Path>,
    pub sleep_left: Option<Duration>,
}

impl<'a> Scene<'a> {
    #[must_use]
    pub fn from_model(model: &'a Model, presentation: ScenePresentation<'a>) -> Self {
        Self {
            player: &model.player,
            transport: &model.transport,
            playlist: &model.playlist,
            queue: &model.queue,
            favorites: &model.favorites,
            themes: &model.themes,
            appearance_settings: &model.appearance_settings,
            settings: &model.settings,
            revisions: &model.revisions,
            overlay: model.workspace.overlay.as_ref(),
            history: &model.history,
            toasts: &model.workspace.toasts,
            browse_selected: ViewIndex::new(model.workspace.browse.selected().get()),
            playing: model.playing_index(),
            displayed_track: model.displayed_track(),
            library_loading: if model.library.is_none() {
                LibraryLoad::Loading
            } else {
                LibraryLoad::Ready
            },
            scan: model.scan_status,
            theme: presentation.theme,
            color_depth: presentation.color_depth,
            bindings: model.workspace.keymap.bindings(),
            spectrum: presentation.spectrum,
            pixel_path: presentation.pixel_path,
            cell_aspect: presentation.cell_aspect,
            clock: presentation.clock,
            now: presentation.now,
            music_dir: &model.music_dir,
            home: presentation.home,
            sleep_left: presentation.sleep_left,
        }
    }

    #[must_use]
    pub fn current_track_path(&self) -> Option<&'a Path> {
        self.player.current().map(|track| track.path())
    }

    #[must_use]
    pub fn active_theme(&self) -> ActiveTheme<'a> {
        ActiveTheme::new(self.theme, self.color_depth)
            .with_progress(self.appearance().progress)
    }

    pub fn appearance(&self) -> Appearance {
        self.settings.appearance
    }

    #[must_use]
    pub(crate) fn key_hints(&self) -> KeyHintsView<'a> {
        match self.overlay {
            Some(Overlay::Settings(..)) => KeyHintsView::settings(self.bindings),
            None
            | Some(
                Overlay::Help
                | Overlay::Search(_)
                | Overlay::SavePlaylist { .. }
                | Overlay::History(_)
                | Overlay::ConfirmDelete(_)
                | Overlay::JumpToTime(_)
                | Overlay::TrackDetails(_)
                | Overlay::MusicDir { .. },
            ) => KeyHintsView::keys(self.bindings),
        }
    }

    #[must_use]
    pub fn cover_mode(&self) -> CoverMode {
        painted_cover_mode(self.appearance().settings.cover_mode, self.pixel_path)
    }

    #[must_use]
    pub fn cover_sizing(&self) -> CoverSizing {
        cover_sizing(self.cover_mode(), self.appearance().cover_cells)
    }

    #[must_use]
    pub fn on_screen(&self, layout: &FrameLayout) -> OnScreen {
        OnScreen {
            progress_bar: self.progress_bar_width(layout).map(Cells),
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
            self.cover_mode() == CoverMode::Milkdrop && layout.cover.is_some();
        card_spectrum || milkdrop_spectrum
    }

    fn progress_bar_width(&self, layout: &FrameLayout) -> Option<u16> {
        match layout.breakpoint {
            Breakpoint::Full => layout
                .card
                .map(|metrics| self.full_progress_bar_width(&metrics)),
            Breakpoint::Compact => Some(compact_progress_bar_width(layout.header)),
            Breakpoint::Minimal => Some(
                minimal_progress_bar_width(
                    CardView::from_scene(self),
                    self.appearance().settings.speed_chip,
                    Cells(layout.screen.width),
                )
                .0,
            ),
            Breakpoint::TooSmall => None,
        }
    }

    fn full_progress_bar_width(&self, metrics: &CardMetrics) -> u16 {
        let row_width = metrics.progress_row.width;
        match self.appearance().settings.progress_time {
            ProgressTime::Remaining => hud_progress_bar_width(
                row_width,
                CardView::from_scene(self).remaining(),
            ),
            ProgressTime::Elapsed => row_width,
        }
    }
}

pub(crate) fn theme_label(choice: &ThemeChoice) -> &str {
    match choice {
        ThemeChoice::Auto => "auto",
        ThemeChoice::Named(name) => name.as_str(),
    }
}

fn painted_cover_mode(style: CoverMode, detected: PixelPath) -> CoverMode {
    match (style, detected) {
        (CoverMode::Vinyl | CoverMode::Plain, PixelPath::Halfblocks)
        | (CoverMode::Off, PixelPath::Protocol | PixelPath::Halfblocks) => {
            CoverMode::Off
        }
        (CoverMode::Vinyl | CoverMode::Plain, PixelPath::Protocol)
        | (CoverMode::Milkdrop, PixelPath::Protocol | PixelPath::Halfblocks) => style,
    }
}

#[must_use]
pub(crate) fn abbreviate_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::domain::{
        appearance::{CoverMode, ProgressTime},
        geometry::Cells,
    };
    use ratatui::layout::{Rect, Size};
    use rstest::rstest;

    use crate::{
        card::{CardView, compact_progress_bar_width},
        primitive::bar::hud_progress_bar_width,
        repaint::Presence,
        scene::{PixelPath, abbreviate_home, painted_cover_mode},
        screen::{Breakpoint, FrameLayout},
        test_support::{SceneSources, model_with_tracks},
    };

    #[rstest]
    #[case::vinyl_with_graphics(
        CoverMode::Vinyl,
        PixelPath::Protocol,
        CoverMode::Vinyl
    )]
    #[case::plain_with_graphics(
        CoverMode::Plain,
        PixelPath::Protocol,
        CoverMode::Plain
    )]
    #[case::vinyl_without_graphics(
        CoverMode::Vinyl,
        PixelPath::Halfblocks,
        CoverMode::Off
    )]
    #[case::plain_without_graphics(
        CoverMode::Plain,
        PixelPath::Halfblocks,
        CoverMode::Off
    )]
    #[case::off_with_graphics(CoverMode::Off, PixelPath::Protocol, CoverMode::Off)]
    #[case::off_without_graphics(CoverMode::Off, PixelPath::Halfblocks, CoverMode::Off)]
    #[case::milkdrop_with_graphics(
        CoverMode::Milkdrop,
        PixelPath::Protocol,
        CoverMode::Milkdrop
    )]
    #[case::milkdrop_without_graphics(
        CoverMode::Milkdrop,
        PixelPath::Halfblocks,
        CoverMode::Milkdrop
    )]
    fn the_painted_cover_mode_reads_the_style_and_the_terminal(
        #[case] style: CoverMode,
        #[case] detected: PixelPath,
        #[case] expected: CoverMode,
    ) {
        assert_eq!(painted_cover_mode(style, detected), expected);
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
        sources.appearance_mut().breakpoints.min_columns = Cells(min_columns);
        sources.appearance_mut().breakpoints.min_rows = Cells(min_rows);
        if let Some(style) = style {
            sources.appearance_mut().settings.progress_time = style;
        }
        let scene = sources.scene();
        let layout =
            FrameLayout::from_scene(&scene, Rect::new(0, 0, size.width, size.height));
        let on_screen = scene.on_screen(&layout);

        match layout.breakpoint {
            Breakpoint::Full => {
                let metrics = layout.card.unwrap();
                let row_width = metrics.progress_row.width;
                let expected = match style.unwrap() {
                    ProgressTime::Remaining => hud_progress_bar_width(
                        row_width,
                        CardView::from_scene(&scene).remaining(),
                    ),
                    ProgressTime::Elapsed => row_width,
                };
                assert_eq!(on_screen.progress_bar, Some(Cells(expected)));
                assert_eq!(on_screen.clock, Presence::Shown);
            }
            Breakpoint::Compact => {
                assert_eq!(
                    on_screen.progress_bar,
                    Some(Cells(compact_progress_bar_width(layout.header)))
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
