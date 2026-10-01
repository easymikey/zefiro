use config::CoverStyle;
use kernel::{
    domain::{Model, Moment, Overlay, SavePhase, SettingRow},
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
        confirm_delete,
        help::HelpOverlay,
        history::HistoryOverlay,
        jump_to_time,
        modal::{OverlayAreas, OverlayContainer, Prompt},
        music_dir,
        search::SearchOverlay,
        settings::{SettingsOverlay, SettingsView},
        track_details::TrackDetailsOverlay,
    },
    primitive::canvas::Canvas,
    screen::FrameLayout,
    theme::{ActiveTheme, Role},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayContent<'a> {
    pub(crate) model: &'a Model,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) settings_view: SettingsView<'a>,
    pub(crate) bindings: &'a [KeyBinding],
    pub(crate) now: Moment,
}

#[derive(Debug)]
pub(crate) struct OverlayLayer<'a> {
    content: OverlayContent<'a>,
    layout: &'a FrameLayout,
    avoid: Option<Rect>,
}

#[derive(Debug)]
enum ActiveOverlay<'a> {
    Help(HelpOverlay<'a>),
    Search(SearchOverlay<'a>),
    History(HistoryOverlay<'a>),
    Settings(SettingsOverlay<'a>),
    Prompt(Prompt<'a>),
    TrackDetails(TrackDetailsOverlay<'a>),
}

impl ActiveOverlay<'_> {
    fn areas(&self, screen: Rect) -> OverlayAreas {
        match self {
            Self::Help(overlay) => overlay.areas(screen),
            Self::Search(overlay) => overlay.areas(screen),
            Self::History(overlay) => overlay.areas(screen),
            Self::Settings(overlay) => overlay.areas(screen),
            Self::Prompt(prompt) => OverlayAreas::Dialog(prompt.areas(screen)),
            Self::TrackDetails(overlay) => overlay.areas(screen),
        }
    }

    fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        match self {
            Self::Help(overlay) => overlay.render_in(areas, canvas),
            Self::Search(overlay) => overlay.render_in(areas, canvas),
            Self::History(overlay) => overlay.render_in(areas, canvas),
            Self::Settings(overlay) => overlay.render_in(areas, canvas),
            Self::Prompt(prompt) => {
                if let OverlayAreas::Dialog(areas) = areas {
                    prompt.render_in(areas, canvas);
                }
            }
            Self::TrackDetails(overlay) => overlay.render_in(areas, canvas),
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
        SavePhase::Prompt => theme.role(Role::Accent),
        SavePhase::Failure => theme.role(Role::Accent2),
    }
}

impl<'a> OverlayLayer<'a> {
    #[must_use]
    pub(crate) fn placed(
        content: OverlayContent<'a>,
        layout: &'a FrameLayout,
        cover_style: CoverStyle,
    ) -> Self {
        Self {
            content,
            layout,
            avoid: layout.cover_exclusion(cover_style),
        }
    }

    fn container(&self, avoid: &'a [Rect]) -> OverlayContainer<'a> {
        if self.layout.playlist_pane.is_empty() {
            OverlayContainer::Modal { avoid }
        } else {
            OverlayContainer::Pane(self.layout.playlist_pane)
        }
    }

    fn active(&'a self) -> Option<ActiveOverlay<'a>> {
        let avoid = self.avoid.as_slice();
        match self.content.model.workspace.overlay.as_ref()? {
            Overlay::Help => Some(ActiveOverlay::Help(HelpOverlay {
                theme: self.content.theme,
                bindings: self.content.bindings,
                avoid,
            })),
            Overlay::Search(search) => Some(ActiveOverlay::Search(SearchOverlay {
                theme: self.content.theme,
                tracks: &self.content.model.playlist.tracks,
                search,
                bounds: self.layout.search_bounds,
                container: self.container(avoid),
            })),
            Overlay::History(cursor) => Some(ActiveOverlay::History(HistoryOverlay {
                theme: self.content.theme,
                entries: &self.content.model.history,
                now: self.content.now,
                selected: cursor.selected(),
                container: self.container(avoid),
            })),
            Overlay::Settings { selected: current } => {
                let rows = SettingRow::all(self.content.settings_view.custom_settings);
                let selected =
                    rows.iter().position(|row| *row == *current).unwrap_or(0);
                Some(ActiveOverlay::Settings(SettingsOverlay {
                    theme: self.content.theme,
                    values: self.content.settings_view,
                    selected,
                    avoid,
                }))
            }
            overlay @ (Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir { .. }
            | Overlay::SavePlaylist { .. }) => self.dialog(overlay, avoid),
        }
    }

    fn dialog(
        &'a self,
        overlay: &'a Overlay,
        avoid: &'a [Rect],
    ) -> Option<ActiveOverlay<'a>> {
        match overlay {
            Overlay::ConfirmDelete(candidate) => Some(ActiveOverlay::Prompt(
                confirm_delete::prompt(candidate, self.content.theme).avoiding(avoid),
            )),
            Overlay::JumpToTime(digits) => Some(ActiveOverlay::Prompt(
                jump_to_time::prompt(digits, self.content.theme).avoiding(avoid),
            )),
            Overlay::TrackDetails(track) => {
                Some(ActiveOverlay::TrackDetails(TrackDetailsOverlay {
                    track: track.as_ref(),
                    theme: self.content.theme,
                    avoid,
                }))
            }
            Overlay::MusicDir { typed, error } => Some(ActiveOverlay::Prompt(
                music_dir::prompt(typed, error.as_ref(), self.content.theme)
                    .avoiding(avoid),
            )),
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::History(_)
            | Overlay::Settings { .. }
            | Overlay::SavePlaylist { .. } => None,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        if self.content.model.workspace.save_line().is_some() {
            return banner_area(screen).map(OverlayAreas::Banner);
        }
        Some(self.active()?.areas(screen))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        if let Some(save_line) = self.content.model.workspace.save_line() {
            if let OverlayAreas::Banner(banner) = areas {
                Paragraph::new(save_line.text.as_str())
                    .style(
                        Style::default()
                            .fg(banner_color(save_line.phase, self.content.theme)),
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
        Model,
        Moment,
        Overlay,
        PlaylistIndex,
        SearchQuery,
        SettingRow,
        TextEntry,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{
            layer::{OverlayContent, OverlayLayer},
            modal::OverlayAreas,
            settings::test_support::{custom_settings, settings_values},
        },
        screen::{Breakpoint, FrameLayout},
        test_support::{noir, rendered},
        theme::{ActiveTheme, ColorDepth},
    };

    fn model_with(overlay: Overlay) -> Model {
        let mut model = Model::default();
        model.workspace.overlay = Some(overlay);
        model
    }

    fn layout(playlist_pane: Rect) -> FrameLayout {
        FrameLayout {
            screen: Rect::new(0, 0, 80, 28),
            breakpoint: Breakpoint::Full,
            content: Rect::default(),
            header: Rect::default(),
            card: None,
            cover: None,
            playlist_pane,
            playlist: None,
            key_hints: None,
            search_bounds: Rect::new(0, 0, 80, 28),
            overlay: None,
            toast: None,
        }
    }

    fn layer<'a>(
        theme: &'a crate::theme::Theme,
        model: &'a Model,
        layout: &'a FrameLayout,
    ) -> OverlayLayer<'a> {
        OverlayLayer {
            content: OverlayContent {
                model,
                theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
                settings_view: settings_values(&[]),
                bindings: &[],
                now: Moment::default(),
            },
            layout,
            avoid: None,
        }
    }

    #[test]
    fn no_overlay_paints_nothing() {
        let theme = noir();
        let model = Model::default();
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        assert_eq!(overlay.areas(Rect::new(0, 0, 80, 28)), None);
    }

    #[test]
    fn help_overlay_is_painted_over_the_screen() {
        let theme = noir();
        let model = model_with(Overlay::Help);
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let screen = Rect::new(0, 0, 80, 28);
        assert!(overlay.areas(screen).is_some());
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn search_overlay_uses_the_playlist_pane_when_one_is_given() {
        let theme = noir();
        let model =
            model_with(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
        let layout = layout(Rect::new(0, 0, 80, 28));
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn history_overlay_falls_back_to_a_modal_without_a_playlist_pane() {
        let theme = noir();
        let model = model_with(Overlay::History(CursorOver::new((), 0)));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn settings_overlay_lists_the_settings_view() {
        let theme = noir();
        let custom = custom_settings();
        let model = model_with(Overlay::Settings {
            selected: SettingRow::first(&custom),
        });
        let layout = layout(Rect::default());
        let mut with_values = layer(&theme, &model, &layout);
        with_values.content.settings_view = settings_values(&custom);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame
                .render_widget(&with_values, frame.area()))
            .to_string()
        );
    }

    #[test]
    fn confirm_delete_overlay_shows_the_prompt() {
        let theme = noir();
        let model = model_with(Overlay::ConfirmDelete(DeleteCandidate {
            track: PlaylistIndex::new(0),
            title: "Moon River".to_string(),
            artist: "Audrey Hepburn".to_string(),
        }));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn jump_to_time_overlay_shows_the_prompt() {
        let theme = noir();
        let model =
            model_with(Overlay::JumpToTime(kernel::domain::JumpDigits::default()));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
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
        let model = model_with(Overlay::TrackDetails(track));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn music_dir_overlay_shows_the_prompt() {
        let theme = noir();
        let model = model_with(Overlay::MusicDir {
            typed: TextEntry::default(),
            error: None,
        });
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[rstest]
    #[case::prompt(None)]
    #[case::failure(Some(kernel::domain::playlist::PlaylistNameError::Empty))]
    fn save_playlist_shows_the_bottom_banner(
        #[case] error: Option<kernel::domain::playlist::PlaylistNameError>,
    ) {
        let theme = noir();
        let model = model_with(Overlay::SavePlaylist {
            typed: TextEntry {
                input: "mixtape".to_string(),
            },
            error,
        });
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let screen = Rect::new(0, 0, 80, 28);
        assert_eq!(
            overlay.areas(screen).map(OverlayAreas::outer),
            Some(Rect::new(0, screen.height - 1, screen.width, 1))
        );
    }

    #[test]
    fn overlay_layer_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let model = model_with(Overlay::Help);
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let _ = rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()))
            .to_string();
    }
}
