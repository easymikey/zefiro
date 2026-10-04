use std::sync::Arc;

use kernel::{
    domain::{
        HistoryEntry,
        Moment,
        Overlay,
        SaveLine,
        SavePhase,
        SettingRow,
        Track,
        ViewIndex,
        appearance::CoverMode,
    },
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
        help::HelpWidget,
        history::HistoryWidget,
        jump_to_time,
        modal::{ModalRowStyle, OverlayAreas, OverlayContainer, PromptWidget},
        music_dir,
        search::SearchWidget,
        settings::{SettingsView, SettingsWidget},
        track_details::TrackDetailsWidget,
    },
    primitive::canvas::Canvas,
    scene::Scene,
    screen::FrameLayout,
    theme::{ActiveTheme, Role},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayView<'a> {
    pub(crate) overlay: Option<&'a Overlay>,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) settings_view: SettingsView<'a>,
    pub(crate) bindings: &'a [KeyBinding],
    pub(crate) now: Moment,
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
            now: scene.now,
        }
    }
}

#[derive(Debug)]
pub(crate) struct OverlayWidget<'a> {
    content: OverlayView<'a>,
    layout: &'a FrameLayout,
    avoid: Option<Rect>,
}

#[derive(Debug)]
enum ActiveOverlay<'a> {
    Help(HelpWidget<'a>),
    Search(SearchWidget<'a>),
    History(HistoryWidget<'a>),
    Settings(SettingsWidget<'a>),
    Prompt(PromptWidget<'a>),
    TrackDetails(TrackDetailsWidget<'a>),
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

    fn paint(&self, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        match self {
            Self::Help(widget) => Widget::render(widget, area, buffer),
            Self::Search(widget) => Widget::render(widget, area, buffer),
            Self::History(widget) => Widget::render(widget, area, buffer),
            Self::Settings(widget) => {
                Widget::render(widget, area, buffer);
            }
            Self::Prompt(prompt) => Widget::render(prompt, area, buffer),
            Self::TrackDetails(widget) => {
                Widget::render(widget, area, buffer);
            }
        }
    }
}

fn banner_area(screen: Rect) -> Option<Rect> {
    if screen.height == 0 {
        return None;
    }
    Some(Rect::new(0, screen.height - 1, screen.width, 1))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SaveBannerStyle {
    accent: ratatui::style::Color,
    alert: ratatui::style::Color,
}

impl SaveBannerStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            accent: theme.role(Role::Accent),
            alert: theme.role(Role::Accent2),
        }
    }

    fn color(self, phase: SavePhase) -> ratatui::style::Color {
        match phase {
            SavePhase::Prompt => self.accent,
            SavePhase::Failed => self.alert,
        }
    }
}

impl<'a> OverlayWidget<'a> {
    #[must_use]
    pub(crate) fn placed(
        content: OverlayView<'a>,
        layout: &'a FrameLayout,
        cover_mode: CoverMode,
    ) -> Self {
        Self {
            content,
            layout,
            avoid: layout.cover_exclusion(cover_mode),
        }
    }

    fn container(&self, avoid: &'a [Rect]) -> OverlayContainer<'a> {
        if self.layout.playlist_pane.is_empty() {
            OverlayContainer::Modal(avoid)
        } else {
            OverlayContainer::Pane(self.layout.playlist_pane)
        }
    }

    fn active(&'a self) -> Option<ActiveOverlay<'a>> {
        let avoid = self.avoid.as_slice();
        match self.content.overlay? {
            Overlay::Help => Some(ActiveOverlay::Help(HelpWidget {
                theme: self.content.theme,
                bindings: self.content.bindings,
                avoid,
            })),
            Overlay::Search(search) => Some(ActiveOverlay::Search(SearchWidget {
                theme: self.content.theme,
                tracks: self.content.tracks,
                search,
                bounds: self.layout.search_bounds,
                container: self.container(avoid),
            })),
            Overlay::History(cursor) => Some(ActiveOverlay::History(HistoryWidget {
                theme: self.content.theme,
                entries: self.content.history,
                now: self.content.now,
                selected: ViewIndex::new(usize::from(cursor.selected())),
                container: self.container(avoid),
            })),
            Overlay::Settings(current) => {
                let rows =
                    SettingRow::all(self.content.settings_view.appearance_settings);
                let selected =
                    rows.iter().position(|row| *row == *current).unwrap_or(0);
                Some(ActiveOverlay::Settings(SettingsWidget {
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
                Some(ActiveOverlay::TrackDetails(TrackDetailsWidget {
                    track: track.as_ref(),
                    style: ModalRowStyle::from_theme(&self.content.theme),
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
            | Overlay::Settings(..)
            | Overlay::SavePlaylist { .. } => None,
        }
    }

    fn save_line(&self) -> Option<SaveLine> {
        self.content.overlay.and_then(Overlay::save_line)
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        if self.save_line().is_some() {
            return banner_area(screen).map(OverlayAreas::Banner);
        }
        Some(self.active()?.areas(screen))
    }

    fn paint_banner(&self, save_line: SaveLine, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        if let Some(banner) = banner_area(area) {
            Paragraph::new(save_line.text)
                .style(
                    Style::default()
                        .fg(SaveBannerStyle::from_theme(&self.content.theme)
                            .color(save_line.phase)),
                )
                .render(banner, buffer);
        }
    }
}

impl Widget for &OverlayWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(save_line) = self.save_line() {
            self.paint_banner(save_line, Canvas { area, buffer });
            return;
        }
        if let Some(overlay) = self.active() {
            overlay.paint(Canvas { area, buffer });
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
        SearchQuery,
        SettingRow,
        TextEntry,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{
            layer::{OverlayView, OverlayWidget},
            modal::OverlayAreas,
            settings::test_support::{appearance_settings, settings_values},
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
    ) -> OverlayWidget<'a> {
        OverlayWidget {
            content: OverlayView {
                overlay: model.workspace.overlay.as_ref(),
                tracks: &model.playlist.tracks,
                history: &model.history,
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
        let custom = appearance_settings();
        let model = model_with(Overlay::Settings(SettingRow::first(&custom)));
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
            source: kernel::TrackRef::Local("/music/moon.flac".into()),
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
        let frame = rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()))
            .to_string();
        assert_eq!(frame.lines().count(), 3);
    }
}
