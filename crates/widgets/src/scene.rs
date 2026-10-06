use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    domain::{
        appearance::{Appearance, CoverMode},
        device::DeviceName,
        favorites::Favorites,
        history::HistoryEntry,
        index::ViewIndex,
        model::{Model, ScanStatus},
        overlay::Overlay,
        player::Player,
        playlist::Playlist,
        revision::Revisions,
        settings::Settings,
        startup::Shuffle,
        theme::{ThemeChoice, Themes},
        time::Moment,
        toast::Toast,
        track::{Track, TrackRef},
        transport::Transport,
    },
    update::keymap::chord::KeyBinding,
};

use crate::{
    card::CardView,
    geometry::{CoverSizing, cover_sizing},
    key_hints::{KeyHintChords, KeyHintsView},
    overlay::{layer::OverlayView, settings::view::SettingsView},
    playlist::view::{LibraryLoad, PlaylistView},
    spectrum::Spectrum,
    status_line::StatusLineView,
    theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
    toast::ToastWidget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelPath {
    Protocol,
    Halfblocks,
}

#[derive(Debug, Clone, Copy)]
pub struct ScenePresentation<'a> {
    pub appearance: &'a Appearance,
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    pub spectrum: &'a Spectrum,
    pub pixel_path: PixelPath,
    pub cell_aspect: f32,
    pub clock: Duration,
    pub now: Moment,
    pub home: Option<&'a Path>,
    pub key_hint_chords: &'a KeyHintChords,
}

#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    pub player: &'a Player,
    pub transport: &'a Transport,
    pub playlist: &'a Playlist,
    pub queue: &'a [TrackRef],
    pub favorites: &'a Favorites,
    pub themes: &'a Themes,
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
    pub bindings: &'a [KeyBinding],
    pub music_dir: &'a Path,
    pub presentation: ScenePresentation<'a>,
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
            bindings: model.workspace.keymap.bindings(),
            music_dir: &model.music_dir,
            presentation,
        }
    }

    #[must_use]
    pub fn current_track_path(&self) -> Option<&'a Path> {
        self.player.current().map(|track| track.path())
    }

    #[must_use]
    pub(crate) fn active_theme(&self) -> ActiveTheme<'a> {
        ActiveTheme::new(self.presentation.theme, self.presentation.color_depth)
            .with_progress(self.presentation.appearance.progress)
    }

    #[must_use]
    pub fn cover_mode(&self) -> CoverMode {
        painted_cover_mode(
            self.settings.appearance.cover_mode,
            self.presentation.pixel_path,
        )
    }

    #[must_use]
    pub(crate) fn cover_sizing(&self) -> CoverSizing {
        cover_sizing(self.cover_mode(), self.presentation.appearance.cover_cells)
    }
}

impl<'a> KeyHintsView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let chords = scene.presentation.key_hint_chords;
        match scene.overlay {
            Some(Overlay::Settings(..)) => Self {
                full: &chords.settings,
                compact: &chords.settings,
            },
            None
            | Some(
                Overlay::Help
                | Overlay::Search(_)
                | Overlay::SavePlaylist(_)
                | Overlay::History(_)
                | Overlay::ConfirmDelete(_)
                | Overlay::JumpToTime(_)
                | Overlay::TrackDetails(_)
                | Overlay::MusicDir(_),
            ) => Self {
                full: &chords.keys,
                compact: &chords.compact,
            },
        }
    }
}

impl<'a> CardView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            player: scene.player,
            speed: scene.transport.speed,
            volume: scene.transport.volume,
            spectrum: scene.presentation.spectrum,
            repeat: scene.playlist.repeat,
            play_order: &scene.playlist.play_order,
            displayed_track: scene.displayed_track,
            output: &scene.transport.output,
            now: scene.presentation.now,
        }
    }
}

impl<'a> StatusLineView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let shuffle = if scene.playlist.play_order.is_shuffle() {
            Shuffle::Enabled
        } else {
            Shuffle::Disabled
        };
        Self {
            shuffle,
            repeat_mode: scene.playlist.repeat,
            queue_len: scene.queue.len(),
            position: scene.browse_selected,
            total: scene.playlist.tracks.len(),
            scan_status: scene.scan,
            scanning_label: scene.presentation.theme.scanning_label.as_str(),
            theme_name: scene.presentation.theme.name.as_str(),
            sleep_left: scene
                .transport
                .sleep
                .map(|timer| timer.deadline.elapsed_since(scene.presentation.now)),
        }
    }
}

impl<'a> PlaylistView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            playlist: scene.playlist,
            queue: scene.queue,
            favorites: scene.favorites,
            browse_selected: scene.browse_selected.get(),
            playing: scene.playing,
            library_loading: scene.library_loading,
            status: StatusLineView::from_scene(scene),
        }
    }
}

impl<'a> SettingsView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let audio = &scene.settings.audio;
        Self {
            crossfade: audio.crossfade,
            replay_gain: audio.replay_gain,
            theme: theme_label(&scene.themes.selected),
            themes: &scene.themes.names,
            sleep_presets: audio.sleep_presets.as_slice(),
            music_dir: scene.music_dir,
            home: scene.presentation.home,
            output_device: audio.device.named().map(DeviceName::as_str),
            output_devices: &scene.settings.output_devices,
            appearance: scene.settings.appearance,
        }
    }
}

impl<'a> OverlayView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            overlay: scene.overlay,
            tracks: &scene.playlist.tracks,
            history: scene.history,
            theme: scene.active_theme(),
            settings_view: SettingsView::from_scene(scene),
            bindings: scene.bindings,
            now: scene.presentation.now,
        }
    }
}

impl<'a> ToastWidget<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Option<Self> {
        (!scene.toasts.is_empty())
            .then(|| Self::new(scene.toasts, scene.active_theme()))
    }
}

fn theme_label(choice: &ThemeChoice) -> &str {
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

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::CoverMode;
    use rstest::rstest;

    use crate::scene::{PixelPath, painted_cover_mode};

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
}
