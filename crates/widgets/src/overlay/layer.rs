use std::sync::Arc;

use kernel::{
    domain::{
        history::HistoryEntry,
        index::RowIndex,
        overlay::{Overlay, TextEntry},
        time::Moment,
        track::Track,
    },
    update::keymap::chord::KeyBinding,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        confirm_trash,
        help::HelpWidget,
        history::HistoryWidget,
        jump_to_time,
        modal::{
            placement::{ModalContainer, OverlayAreas},
            prompt::PromptWidget,
        },
        music_dir,
        search::SearchWidget,
        settings::{SettingsWidget, view::SettingsView},
        track_details::TrackDetailsWidget,
    },
    primitive::canvas::Canvas,
    screen::frame_layout::FrameLayout,
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayView<'a> {
    pub(crate) overlay: Option<&'a Overlay>,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) active_theme: ActiveTheme<'a>,
    pub(crate) settings_view: SettingsView<'a>,
    pub(crate) bindings: &'a [KeyBinding],
    pub(crate) now: Moment,
}

#[derive(Debug)]
pub(crate) struct OverlayWidget<'a> {
    view: OverlayView<'a>,
    frame_layout: &'a FrameLayout,
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
            Self::Prompt(overlay) => OverlayAreas::Dialog(overlay.areas(screen)),
            Self::TrackDetails(overlay) => overlay.areas(screen),
        }
    }

    fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        match self {
            Self::Help(widget) => widget.paint(areas, canvas),
            Self::Search(widget) => widget.paint(areas, canvas),
            Self::Settings(widget) => widget.paint(areas, canvas),
            Self::History(widget) => widget.paint(areas, canvas),
            Self::TrackDetails(widget) => widget.paint(areas, canvas),
            Self::Prompt(widget) => match areas {
                OverlayAreas::Dialog(dialog) => widget.paint(dialog, canvas),
                OverlayAreas::List(_) | OverlayAreas::Banner(_) => {}
            },
        }
    }
}

fn banner_area(screen: Rect) -> Option<Rect> {
    if screen.height == 0 {
        return None;
    }
    Some(Rect {
        y: screen.bottom() - 1,
        height: 1,
        ..screen
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavePhase {
    Typing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaveLine {
    text: String,
    phase: SavePhase,
}

impl SaveLine {
    fn from_overlay(overlay: &Overlay) -> Option<Self> {
        match overlay {
            Overlay::SavePlaylist(TextEntry { input, error: None }) => Some(Self {
                text: format!("Save playlist: {input}"),
                phase: SavePhase::Typing,
            }),
            Overlay::SavePlaylist(TextEntry {
                error: Some(reason),
                ..
            }) => Some(Self {
                text: reason.to_string(),
                phase: SavePhase::Failed,
            }),
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::ConfirmTrash(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir(_) => None,
        }
    }
}

#[must_use]
fn accent(active_theme: &ActiveTheme<'_>, phase: SavePhase) -> ratatui::style::Color {
    match phase {
        SavePhase::Typing => active_theme.colors().accent,
        SavePhase::Failed => active_theme.alert(),
    }
}

impl<'a> OverlayWidget<'a> {
    #[must_use]
    pub(crate) fn new(view: OverlayView<'a>, frame_layout: &'a FrameLayout) -> Self {
        Self {
            view,
            frame_layout,
            avoid: None,
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: Option<Rect>) -> Self {
        self.avoid = avoid;
        self
    }

    fn container(&self, avoid: &'a [Rect]) -> ModalContainer<'a> {
        if self.frame_layout.playlist_pane.is_empty() {
            ModalContainer::Floating(avoid)
        } else {
            ModalContainer::Playlist(self.frame_layout.playlist_pane)
        }
    }

    fn active(&'a self) -> Option<ActiveOverlay<'a>> {
        let avoid = self.avoid.as_slice();
        match self.view.overlay? {
            Overlay::Help => Some(ActiveOverlay::Help(
                HelpWidget::new(self.view.bindings, self.view.active_theme)
                    .avoid(avoid),
            )),
            Overlay::Search(search) => Some(ActiveOverlay::Search(
                SearchWidget::new(search, self.view.active_theme)
                    .tracks(self.view.tracks)
                    .bounds(self.frame_layout.search_bounds)
                    .container(self.container(avoid)),
            )),
            Overlay::History(cursor) => Some(ActiveOverlay::History(
                HistoryWidget::new(self.view.history, self.view.active_theme)
                    .now(self.view.now)
                    .selected(RowIndex::new(usize::from(cursor.selected())))
                    .container(self.container(avoid)),
            )),
            Overlay::Settings(current) => Some(ActiveOverlay::Settings(
                SettingsWidget::new(
                    self.view.settings_view,
                    *current,
                    self.view.active_theme,
                )
                .avoid(avoid),
            )),
            overlay @ (Overlay::ConfirmTrash(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir(_)
            | Overlay::SavePlaylist(_)) => self.dialog(overlay, avoid),
        }
    }

    fn dialog(
        &'a self,
        overlay: &'a Overlay,
        avoid: &'a [Rect],
    ) -> Option<ActiveOverlay<'a>> {
        match overlay {
            Overlay::ConfirmTrash(candidate) => Some(ActiveOverlay::Prompt(
                confirm_trash::prompt(candidate, self.view.active_theme).avoid(avoid),
            )),
            Overlay::JumpToTime(entry) => Some(ActiveOverlay::Prompt(
                jump_to_time::prompt(entry, self.view.active_theme).avoid(avoid),
            )),
            Overlay::TrackDetails(track) => Some(ActiveOverlay::TrackDetails(
                TrackDetailsWidget::new(track.as_ref(), self.view.active_theme)
                    .avoid(avoid),
            )),
            Overlay::MusicDir(entry) => Some(ActiveOverlay::Prompt(
                music_dir::prompt(entry, self.view.active_theme).avoid(avoid),
            )),
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::SavePlaylist(_) => None,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        let overlay = self.view.overlay?;
        if let Overlay::SavePlaylist(_) = overlay {
            return banner_area(screen).map(OverlayAreas::Banner);
        }
        Some(self.active()?.areas(screen))
    }

    fn paint_banner(&self, save_line: SaveLine, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        Paragraph::new(save_line.text)
            .style(
                Style::default().fg(accent(&self.view.active_theme, save_line.phase)),
            )
            .render(area, buffer);
    }
}

impl OverlayWidget<'_> {
    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let Some(overlay) = self.view.overlay else {
            return;
        };
        match SaveLine::from_overlay(overlay) {
            Some(save_line) => match areas {
                OverlayAreas::Banner(banner) => self.paint_banner(
                    save_line,
                    Canvas {
                        area: banner,
                        buffer: canvas.buffer,
                    },
                ),
                OverlayAreas::List(_) | OverlayAreas::Dialog(_) => {}
            },
            None => {
                if let Some(active) = self.active() {
                    active.paint(areas, canvas);
                }
            }
        }
    }
}

impl Widget for &OverlayWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(areas) = self.areas(area) {
            self.paint(areas, Canvas { area, buffer });
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::CoverMode,
        cursor_over::CursorOver,
        model::Model,
        overlay::{Overlay, SearchQuery, TextEntry, TrashCandidate},
        setting_row::SettingRow,
        time::Moment,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{
            layer::{OverlayView, OverlayWidget, SavePhase, accent},
            modal::placement::OverlayAreas,
            settings::test_support::settings_values,
        },
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn model_with(overlay: Overlay) -> Model {
        let mut model = Model::default();
        model.workspace.overlay = Some(overlay);
        model
    }

    fn layout(playlist_pane: Rect) -> FrameLayout {
        FrameLayout {
            playlist_pane,
            search_bounds: Rect::new(0, 0, 80, 28),
            ..FrameLayout::empty(Rect::new(0, 0, 80, 28), Breakpoint::Full)
        }
    }

    fn layer<'a>(
        theme: &'a crate::theme::Theme,
        model: &'a Model,
        layout: &'a FrameLayout,
    ) -> OverlayWidget<'a> {
        OverlayWidget::new(
            OverlayView {
                overlay: model.workspace.overlay.as_ref(),
                tracks: &model.playlist.tracks,
                history: &model.history,
                active_theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
                settings_view: settings_values(),
                bindings: &[],
                now: Moment::default(),
            },
            layout,
        )
        .avoid(layout.cover_exclusion(CoverMode::Vinyl))
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
        let model = model_with(Overlay::Settings(SettingRow::first()));
        let layout = layout(Rect::default());
        let mut with_values = layer(&theme, &model, &layout);
        with_values.view.settings_view = settings_values();
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame
                .render_widget(&with_values, frame.area()))
            .to_string()
        );
    }

    #[test]
    fn confirm_trash_overlay_shows_the_prompt() {
        let theme = noir();
        let model = model_with(Overlay::ConfirmTrash(TrashCandidate {
            source: kernel::domain::track::TrackSource::Local(
                "/music/moon.flac".into(),
            ),
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
        let model = model_with(Overlay::JumpToTime(TextEntry::default()));
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
        let track = std::sync::Arc::new(kernel::domain::track::Track::new(
            kernel::domain::track::TrackParts {
                path: "/music/moon_river.mp3".into(),
                duration: std::time::Duration::from_secs(245),
                tags: kernel::domain::track::Tags::default(),
                audio_format: kernel::domain::track::AudioFormat::default(),
            },
        ));
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
        let model = model_with(Overlay::MusicDir(TextEntry::default()));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[rstest]
    #[case::prompt(None, SavePhase::Typing)]
    #[case::failure(
        Some(kernel::domain::playlist::PlaylistFileNameError::Empty),
        SavePhase::Failed
    )]
    fn save_playlist_shows_the_bottom_banner(
        #[case] error: Option<kernel::domain::playlist::PlaylistFileNameError>,
        #[case] phase: SavePhase,
    ) {
        let theme = noir();
        let text = error
            .as_ref()
            .map_or_else(|| "Save playlist: mixtape".to_string(), ToString::to_string);
        let model = model_with(Overlay::SavePlaylist(TextEntry {
            input: "mixtape".to_string(),
            error,
        }));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let screen = Rect::new(0, 0, 80, 28);
        assert_eq!(
            overlay.areas(screen).map(OverlayAreas::outer),
            Some(Rect {
                y: screen.bottom() - 1,
                height: 1,
                ..screen
            })
        );
        let backend =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()));
        let buffer = backend.buffer();
        let banner = (0..80)
            .map(|x| buffer[(x, 27)].symbol())
            .collect::<String>();
        assert_eq!(banner.trim_end(), text);
        assert_eq!(
            buffer[(0, 27)].fg,
            accent(&ActiveTheme::new(&theme, ColorDepth::TrueColor), phase)
        );
    }

    #[test]
    fn a_dialog_keeps_clear_of_the_cover() {
        let theme = noir();
        let model = model_with(Overlay::JumpToTime(TextEntry::default()));
        let cover_area = Rect::new(0, 0, 80, 15);
        let layout = FrameLayout {
            cover_area: Some(cover_area),
            ..layout(Rect::default())
        };
        let overlay = layer(&theme, &model, &layout);
        let dialog = overlay
            .areas(Rect::new(0, 0, 80, 28))
            .map(OverlayAreas::outer);
        assert!(dialog.is_some_and(|area| !area.intersects(cover_area)));
    }

    #[test]
    fn the_banner_sits_on_the_last_row_of_an_offset_screen() {
        let theme = noir();
        let model = model_with(Overlay::SavePlaylist(TextEntry {
            input: "mixtape".to_string(),
            error: None,
        }));
        let layout = layout(Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let screen = Rect::new(3, 2, 60, 10);
        assert_eq!(
            overlay.areas(screen).map(OverlayAreas::outer),
            Some(Rect::new(3, 11, 60, 1))
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
