use std::sync::Arc;

use kernel::{
    domain::{HistoryEntry, Overlay, SavePhase, Track, Workspace},
    update::keymap::KeyBinding,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        confirm_delete::ConfirmDeleteOverlay,
        help::HelpOverlay,
        history::HistoryOverlay,
        jump_to_time::JumpToTimeOverlay,
        modal::{OverlayAreas, OverlayContainer},
        search::SearchOverlay,
        settings::{SettingsOverlay, SettingsView},
        source_dir::SourceDirOverlay,
        track_details::TrackDetailsOverlay,
    },
    primitive::canvas::Canvas,
    theme::ActiveTheme,
};

#[derive(Debug)]
pub(crate) struct OverlayLayer<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) settings_view: SettingsView<'a>,
    pub(crate) bindings: &'a [KeyBinding],
    pub(crate) avoid: Option<Rect>,
    pub(crate) playlist_pane: Rect,
    pub(crate) search_bounds: Rect,
    pub(crate) now_unix: u64,
}

#[derive(Debug)]
enum ActiveOverlay<'a> {
    Help(HelpOverlay<'a>),
    Search(SearchOverlay<'a>),
    History(HistoryOverlay<'a>),
    Settings(SettingsOverlay<'a>),
    ConfirmDelete(ConfirmDeleteOverlay<'a>),
    JumpToTime(JumpToTimeOverlay<'a>),
    TrackDetails(TrackDetailsOverlay<'a>),
    SourceDir(SourceDirOverlay<'a>),
}

impl ActiveOverlay<'_> {
    fn areas(&self, screen: Rect) -> OverlayAreas {
        match self {
            Self::Help(overlay) => overlay.areas(screen),
            Self::Search(overlay) => overlay.areas(screen),
            Self::History(overlay) => overlay.areas(screen),
            Self::Settings(overlay) => overlay.areas(screen),
            Self::ConfirmDelete(overlay) => overlay.areas(screen),
            Self::JumpToTime(overlay) => overlay.areas(screen),
            Self::TrackDetails(overlay) => overlay.areas(screen),
            Self::SourceDir(overlay) => overlay.areas(screen),
        }
    }

    fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        match self {
            Self::Help(overlay) => overlay.render_in(areas, canvas),
            Self::Search(overlay) => overlay.render_in(areas, canvas),
            Self::History(overlay) => overlay.render_in(areas, canvas),
            Self::Settings(overlay) => overlay.render_in(areas, canvas),
            Self::ConfirmDelete(overlay) => overlay.render_in(areas, canvas),
            Self::JumpToTime(overlay) => overlay.render_in(areas, canvas),
            Self::TrackDetails(overlay) => overlay.render_in(areas, canvas),
            Self::SourceDir(overlay) => overlay.render_in(areas, canvas),
        }
    }
}

fn banner_area(screen: Rect) -> Option<Rect> {
    if screen.height == 0 {
        return None;
    }
    Some(Rect::new(0, screen.height - 1, screen.width, 1))
}

fn banner_color(phase: SavePhase, theme: ActiveTheme<'_>) -> ratatui::style::Color {
    match phase {
        SavePhase::Prompt => theme.accent(),
        SavePhase::Failure => theme.accent2(),
    }
}

impl<'a> OverlayLayer<'a> {
    fn container(&self, avoid: &'a [Rect]) -> OverlayContainer<'a> {
        if self.playlist_pane.is_empty() {
            OverlayContainer::Modal { avoid }
        } else {
            OverlayContainer::Pane(self.playlist_pane)
        }
    }

    fn active(&'a self) -> Option<ActiveOverlay<'a>> {
        let avoid = self.avoid.as_slice();
        match self.workspace.overlay.as_ref()? {
            Overlay::Help => Some(ActiveOverlay::Help(HelpOverlay {
                theme: self.theme,
                bindings: self.bindings,
                avoid,
            })),
            Overlay::Search(search) => Some(ActiveOverlay::Search(SearchOverlay {
                theme: self.theme,
                tracks: self.tracks,
                search,
                bounds: self.search_bounds,
                container: self.container(avoid),
            })),
            Overlay::History(cursor) => Some(ActiveOverlay::History(HistoryOverlay {
                theme: self.theme,
                entries: self.history,
                selected: cursor.selected(),
                now_unix: self.now_unix,
                container: self.container(avoid),
            })),
            Overlay::Settings(cursor) => {
                Some(ActiveOverlay::Settings(SettingsOverlay {
                    theme: self.theme,
                    values: self.settings_view,
                    selected: cursor.selected(),
                    avoid,
                }))
            }
            overlay @ (Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::SourceDir { .. }
            | Overlay::SavePlaylist { .. }) => self.dialog(overlay, avoid),
        }
    }

    fn dialog(
        &'a self,
        overlay: &'a Overlay,
        avoid: &'a [Rect],
    ) -> Option<ActiveOverlay<'a>> {
        match overlay {
            Overlay::ConfirmDelete(candidate) => {
                Some(ActiveOverlay::ConfirmDelete(ConfirmDeleteOverlay {
                    candidate,
                    theme: self.theme,
                    avoid,
                }))
            }
            Overlay::JumpToTime(digits) => {
                Some(ActiveOverlay::JumpToTime(JumpToTimeOverlay {
                    digits,
                    theme: self.theme,
                    avoid,
                }))
            }
            Overlay::TrackDetails(track) => {
                Some(ActiveOverlay::TrackDetails(TrackDetailsOverlay {
                    track: track.as_ref(),
                    theme: self.theme,
                    avoid,
                }))
            }
            Overlay::SourceDir { typed, error } => {
                Some(ActiveOverlay::SourceDir(SourceDirOverlay {
                    typed,
                    error: error.as_ref(),
                    theme: self.theme,
                    avoid,
                }))
            }
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::History(_)
            | Overlay::Settings(_)
            | Overlay::SavePlaylist { .. } => None,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        if self.workspace.save_line().is_some() {
            return banner_area(screen).map(OverlayAreas::Banner);
        }
        Some(self.active()?.areas(screen))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        if let Some(save_line) = self.workspace.save_line() {
            if let OverlayAreas::Banner(banner) = areas {
                Paragraph::new(save_line.text.as_str())
                    .style(
                        Style::default().fg(banner_color(save_line.phase, self.theme)),
                    )
                    .render(banner, canvas.buffer);
            }
            return;
        }
        if let Some(overlay) = self.active() {
            overlay.render_in(areas, canvas);
        }
    }
}

impl Widget for &OverlayLayer<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(areas) = self.areas(area) {
            self.render_in(areas, Canvas { area, buffer });
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        CursorOver,
        DeleteCandidate,
        Overlay,
        PlaylistIndex,
        SearchQuery,
        TextEntry,
        Workspace,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{layer::OverlayLayer, modal::OverlayAreas},
        scene::fixtures::{custom_rows, noir, painted, settings_values},
        theme::{ActiveTheme, ColorDepth},
    };

    fn workspace_with(overlay: Overlay) -> Workspace {
        let mut workspace = Workspace::default();
        workspace.overlay = Some(overlay);
        workspace
    }

    fn layer<'a>(
        theme: &'a crate::theme::Theme,
        workspace: &'a Workspace,
    ) -> OverlayLayer<'a> {
        OverlayLayer {
            workspace,
            theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
            tracks: &[],
            history: &[],
            settings_view: settings_values(&[]),
            bindings: &[],
            avoid: None,
            playlist_pane: Rect::default(),
            search_bounds: Rect::new(0, 0, 80, 28),
            now_unix: 0,
        }
    }

    #[test]
    fn no_overlay_paints_nothing() {
        let theme = noir();
        let workspace = Workspace::default();
        let overlay = layer(&theme, &workspace);
        assert_eq!(overlay.areas(Rect::new(0, 0, 80, 28)), None);
    }

    #[test]
    fn help_overlay_is_painted_over_the_screen() {
        let theme = noir();
        let workspace = workspace_with(Overlay::Help);
        let overlay = layer(&theme, &workspace);
        let screen = Rect::new(0, 0, 80, 28);
        assert!(overlay.areas(screen).is_some());
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn search_overlay_uses_the_playlist_pane_when_one_is_given() {
        let theme = noir();
        let workspace =
            workspace_with(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
        let mut overlay = layer(&theme, &workspace);
        overlay.playlist_pane = Rect::new(0, 0, 80, 28);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn history_overlay_falls_back_to_a_modal_without_a_playlist_pane() {
        let theme = noir();
        let workspace = workspace_with(Overlay::History(CursorOver::new((), 0)));
        let overlay = layer(&theme, &workspace);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn settings_overlay_lists_the_settings_view() {
        let theme = noir();
        let workspace = workspace_with(Overlay::Settings(CursorOver::new(
            kernel::domain::SettingsRows,
            0,
        )));
        let custom = custom_rows();
        let mut with_values = layer(&theme, &workspace);
        with_values.settings_view = settings_values(&custom);
        insta::assert_snapshot!(painted(&with_values, 80, 28));
    }

    #[test]
    fn confirm_delete_overlay_shows_the_prompt() {
        let theme = noir();
        let workspace = workspace_with(Overlay::ConfirmDelete(DeleteCandidate {
            track: PlaylistIndex::new(0),
            title: "Moon River".to_string(),
            artist: "Audrey Hepburn".to_string(),
        }));
        let overlay = layer(&theme, &workspace);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn jump_to_time_overlay_shows_the_prompt() {
        let theme = noir();
        let workspace =
            workspace_with(Overlay::JumpToTime(kernel::domain::JumpDigits::default()));
        let overlay = layer(&theme, &workspace);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn track_details_overlay_shows_the_dialog() {
        let theme = noir();
        let track = std::sync::Arc::new(
            kernel::domain::Track::builder()
                .path("/music/moon_river.mp3")
                .duration(std::time::Duration::from_secs(245))
                .tags(kernel::domain::Tags::default())
                .audio_format(kernel::domain::AudioFormat::default())
                .build(),
        );
        let workspace = workspace_with(Overlay::TrackDetails(track));
        let overlay = layer(&theme, &workspace);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn source_dir_overlay_shows_the_prompt() {
        let theme = noir();
        let workspace = workspace_with(Overlay::SourceDir {
            typed: TextEntry::default(),
            error: None,
        });
        let overlay = layer(&theme, &workspace);
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[rstest]
    #[case::prompt(None)]
    #[case::failure(Some(kernel::domain::playlist::PlaylistNameRejection::Empty))]
    fn save_playlist_shows_the_bottom_banner(
        #[case] error: Option<kernel::domain::playlist::PlaylistNameRejection>,
    ) {
        let theme = noir();
        let workspace = workspace_with(Overlay::SavePlaylist {
            typed: TextEntry {
                input: "mixtape".to_string(),
            },
            error,
        });
        let overlay = layer(&theme, &workspace);
        let screen = Rect::new(0, 0, 80, 28);
        assert_eq!(
            overlay.areas(screen).map(OverlayAreas::painted),
            Some(Rect::new(0, screen.height - 1, screen.width, 1))
        );
    }

    #[test]
    fn overlay_layer_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let workspace = workspace_with(Overlay::Help);
        let overlay = layer(&theme, &workspace);
        let _ = painted(&overlay, 4, 3);
    }
}
