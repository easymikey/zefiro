use std::sync::Arc;

use kernel::{
    domain::{
        history::HistoryEntry,
        index::RowIndex,
        overlay::{Overlay, TextEntry},
        playlist::PlaylistFileNameError,
        server::Server,
        time::Moment,
        track::Track,
    },
    update::keymap::chord::KeyBinding,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        add_server,
        confirm_remove,
        confirm_trash,
        help::{HelpColumns, HelpWidget, groups::HelpGroups},
        history::{HistoryMeasures, HistoryWidget},
        jump_to_time,
        modal::{
            placement::{ModalContainer, OverlayAreas},
            prompt::PromptWidget,
        },
        music_dir,
        search::{SearchWidget, matches::Query, search_title},
        servers::{ServersTable, ServersWidget},
        settings::{SettingsTable, SettingsWidget, view::SettingsView},
        track_details::{TrackDetailsRow, TrackDetailsWidget},
    },
    primitive::canvas::Canvas,
    screen::frame_layout::{FrameLayout, OverlayContent},
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayView<'a> {
    pub(crate) overlay: Option<&'a Overlay>,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) servers: &'a [Server],
    pub(crate) active_theme: ActiveTheme<'a>,
    pub(crate) settings_view: SettingsView<'a>,
    pub(crate) bindings: &'a [KeyBinding],
    pub(crate) now: Moment,
}

impl<'a> OverlayView<'a> {
    #[must_use]
    pub(crate) fn content(&self, screen: Rect) -> Option<OverlayContent<'a>> {
        Some(match self.overlay? {
            Overlay::Help => OverlayContent::Help(HelpColumns::new(
                &HelpGroups::new(self.bindings),
                screen,
            )),
            Overlay::Search(search) => {
                OverlayContent::Search(search, search_title(search, self.tracks.len()))
            }
            Overlay::ServerSearch(server_query) => {
                OverlayContent::ServerSearch(server_query)
            }
            Overlay::SavePlaylist(entry) => OverlayContent::SavePlaylist(entry),
            Overlay::History(cursor) => {
                OverlayContent::History(cursor, HistoryMeasures::of(self.history))
            }
            Overlay::Settings(current) => OverlayContent::Settings(
                *current,
                SettingsTable::new(&self.settings_view),
            ),
            Overlay::ConfirmTrash(track) => {
                OverlayContent::Prompt(confirm_trash::prompt(track, self.active_theme))
            }
            Overlay::JumpToTime(entry) => {
                OverlayContent::Prompt(jump_to_time::prompt(entry, self.active_theme))
            }
            Overlay::TrackDetails(track) => {
                OverlayContent::TrackDetails(TrackDetailsRow::all(track))
            }
            Overlay::MusicDir(entry) => {
                OverlayContent::Prompt(music_dir::prompt(entry, self.active_theme))
            }
            Overlay::AddServer(server_prompt) => OverlayContent::Prompt(
                add_server::prompt(server_prompt, self.active_theme),
            ),
            Overlay::Servers(cursor) => OverlayContent::Servers(
                cursor,
                ServersTable::new(self.servers, cursor.selected(), &self.active_theme),
            ),
            Overlay::ConfirmRemove(server_name) => OverlayContent::Prompt(
                confirm_remove::prompt(server_name, self.active_theme),
            ),
        })
    }
}

#[derive(Debug)]
pub(crate) struct OverlayWidget<'a> {
    view: OverlayView<'a>,
    frame_layout: &'a FrameLayout<'a>,
    avoid: Option<Rect>,
}

#[derive(Debug)]
enum ActiveOverlay<'a> {
    Help(HelpWidget<'a>),
    Search(SearchWidget<'a>),
    History(HistoryWidget<'a>),
    Settings(SettingsWidget<'a>),
    Prompt(&'a PromptWidget<'a>, &'a [Rect]),
    Servers(ServersWidget<'a>),
    TrackDetails(TrackDetailsWidget<'a>),
    Banner(SaveLine<'a>, ActiveTheme<'a>),
}

impl ActiveOverlay<'_> {
    fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        match self {
            Self::Help(overlay) => Some(OverlayAreas::List(overlay.areas(screen))),
            Self::Search(overlay) => Some(overlay.areas(screen)),
            Self::History(overlay) => Some(OverlayAreas::List(overlay.areas(screen))),
            Self::Settings(overlay) => Some(OverlayAreas::List(overlay.areas(screen))),
            Self::Prompt(overlay, avoid) => {
                Some(OverlayAreas::Dialog(overlay.areas(screen, avoid)))
            }
            Self::Servers(overlay) => Some(OverlayAreas::Dialog(overlay.areas(screen))),
            Self::TrackDetails(overlay) => {
                Some(OverlayAreas::Dialog(overlay.areas(screen)))
            }
            Self::Banner(..) => banner_area(screen).map(OverlayAreas::Banner),
        }
    }

    fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        match self {
            Self::Search(widget) => widget.paint(areas, canvas),
            Self::Help(widget) => match areas {
                OverlayAreas::List(list) => widget.paint(list, canvas),
                OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => {}
            },
            Self::Settings(widget) => match areas {
                OverlayAreas::List(list) => widget.paint(list, canvas),
                OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => {}
            },
            Self::History(widget) => match areas {
                OverlayAreas::List(list) => widget.paint(list, canvas),
                OverlayAreas::Dialog(_) | OverlayAreas::Banner(_) => {}
            },
            Self::TrackDetails(widget) => match areas {
                OverlayAreas::Dialog(dialog) => widget.paint(dialog, canvas),
                OverlayAreas::List(_) | OverlayAreas::Banner(_) => {}
            },
            Self::Prompt(widget, _) => match areas {
                OverlayAreas::Dialog(dialog) => widget.paint(dialog, canvas),
                OverlayAreas::List(_) | OverlayAreas::Banner(_) => {}
            },
            Self::Servers(widget) => match areas {
                OverlayAreas::Dialog(dialog) => widget.paint(dialog, canvas),
                OverlayAreas::List(_) | OverlayAreas::Banner(_) => {}
            },
            Self::Banner(save_line, active_theme) => match areas {
                OverlayAreas::Banner(banner) => paint_banner(
                    *save_line,
                    active_theme,
                    Canvas {
                        area: banner,
                        buffer: canvas.buffer,
                    },
                ),
                OverlayAreas::List(_) | OverlayAreas::Dialog(_) => {}
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

#[derive(Debug, Clone, Copy)]
enum SaveLine<'a> {
    Typing(&'a str),
    Failed(&'a PlaylistFileNameError),
}

impl<'a> SaveLine<'a> {
    fn new(text_entry: &'a TextEntry<PlaylistFileNameError>) -> Self {
        text_entry
            .error
            .as_ref()
            .map_or_else(|| Self::Typing(&text_entry.input), Self::Failed)
    }

    fn line(self) -> Line<'a> {
        match self {
            Self::Typing(input) => {
                Line::from(vec![Span::raw("Save playlist: "), Span::raw(input)])
            }
            Self::Failed(reason) => Line::from(reason.to_string()),
        }
    }
}

#[must_use]
fn accent(active_theme: &ActiveTheme<'_>, save_line: SaveLine<'_>) -> Color {
    match save_line {
        SaveLine::Typing(_) => active_theme.colors().accent,
        SaveLine::Failed(_) => active_theme.alert(),
    }
}

fn paint_banner(
    save_line: SaveLine<'_>,
    active_theme: &ActiveTheme<'_>,
    canvas: Canvas<'_>,
) {
    let Canvas { area, buffer } = canvas;
    Paragraph::new(save_line.line())
        .style(Style::default().fg(accent(active_theme, save_line)))
        .render(area, buffer);
}

impl<'a> OverlayWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        view: OverlayView<'a>,
        frame_layout: &'a FrameLayout<'a>,
    ) -> Self {
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
        let theme = self.view.active_theme;
        Some(match self.frame_layout.overlay_content.as_ref()? {
            OverlayContent::Help(help_columns) => {
                ActiveOverlay::Help(HelpWidget::new(help_columns, theme).avoid(avoid))
            }
            OverlayContent::Search(search, title) => ActiveOverlay::Search(
                SearchWidget::new(Query::Search(search), theme)
                    .title(title)
                    .tracks(self.view.tracks)
                    .bounds(self.frame_layout.search_bounds)
                    .container(self.container(avoid)),
            ),
            OverlayContent::ServerSearch(server_query) => ActiveOverlay::Search(
                SearchWidget::new(Query::ServerSearch(server_query), theme)
                    .title(server_query.content.server_name.as_str())
                    .bounds(self.frame_layout.search_bounds)
                    .container(self.container(avoid)),
            ),
            OverlayContent::History(cursor, measures) => ActiveOverlay::History(
                HistoryWidget::new(self.view.history, measures, theme)
                    .now(self.view.now)
                    .selected(RowIndex::new(usize::from(cursor.selected())))
                    .container(self.container(avoid)),
            ),
            OverlayContent::Settings(current, settings_table) => {
                ActiveOverlay::Settings(
                    SettingsWidget::new(self.view.settings_view, settings_table, theme)
                        .selected(*current)
                        .avoid(avoid),
                )
            }
            OverlayContent::TrackDetails(rows) => ActiveOverlay::TrackDetails(
                TrackDetailsWidget::new(rows, theme).avoid(avoid),
            ),
            OverlayContent::SavePlaylist(entry) => {
                ActiveOverlay::Banner(SaveLine::new(entry), theme)
            }
            OverlayContent::Servers(cursor, servers_table) => ActiveOverlay::Servers(
                ServersWidget::new(servers_table, cursor.selected(), theme)
                    .avoid(avoid),
            ),
            OverlayContent::Prompt(prompt_widget) => {
                ActiveOverlay::Prompt(prompt_widget, avoid)
            }
        })
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> Option<OverlayAreas> {
        self.active()?.areas(screen)
    }
}

impl OverlayWidget<'_> {
    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        if let Some(active) = self.active() {
            active.paint(areas, canvas);
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
        model::Model,
        overlay::{Overlay, TextEntry},
        time::Moment,
    };
    use ratatui::layout::Rect;

    use crate::{
        overlay::{
            layer::{OverlayView, OverlayWidget, SaveLine, accent},
            modal::placement::OverlayAreas,
            settings::tests::settings_values,
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

    fn overlay_view<'a>(
        theme: &'a crate::theme::Theme,
        model: &'a Model,
    ) -> OverlayView<'a> {
        OverlayView {
            overlay: model.workspace.overlay.as_ref(),
            tracks: &model.playlist.tracks,
            history: &model.history,
            servers: &model.servers,
            active_theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
            settings_view: settings_values(),
            bindings: &[],
            now: Moment::default(),
        }
    }

    fn layout<'a>(
        theme: &'a crate::theme::Theme,
        model: &'a Model,
        playlist_pane: Rect,
    ) -> FrameLayout<'a> {
        let screen = Rect::new(0, 0, 80, 28);
        FrameLayout {
            playlist_pane,
            search_bounds: screen,
            overlay_content: overlay_view(theme, model).content(screen),
            ..FrameLayout::empty(screen, Breakpoint::Full)
        }
    }

    fn layer<'a>(
        theme: &'a crate::theme::Theme,
        model: &'a Model,
        layout: &'a FrameLayout<'a>,
    ) -> OverlayWidget<'a> {
        OverlayWidget::new(overlay_view(theme, model), layout)
            .avoid(layout.cover_exclusion(CoverMode::Vinyl))
    }

    #[test]
    fn save_playlist_shows_the_bottom_banner() {
        let theme = noir();
        let model = model_with(Overlay::SavePlaylist(TextEntry {
            input: "mixtape".to_string(),
            error: None,
        }));
        let layout = layout(&theme, &model, Rect::default());
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
        assert_eq!(banner.trim_end(), "Save playlist: mixtape");
        assert_eq!(
            buffer[(0, 27)].fg,
            accent(
                &ActiveTheme::new(&theme, ColorDepth::TrueColor),
                SaveLine::Typing("mixtape")
            )
        );
    }

    #[test]
    fn the_banner_sits_on_the_last_row_of_an_offset_screen() {
        let theme = noir();
        let model = model_with(Overlay::SavePlaylist(TextEntry {
            input: "mixtape".to_string(),
            error: None,
        }));
        let layout = layout(&theme, &model, Rect::default());
        let overlay = layer(&theme, &model, &layout);
        let screen = Rect::new(3, 2, 60, 10);
        assert_eq!(
            overlay.areas(screen).map(OverlayAreas::outer),
            Some(Rect::new(3, 11, 60, 1))
        );
    }
}
