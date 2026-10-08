use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Paragraph, Widget},
};

use crate::{
    playlist::{
        catalog::CatalogWidget,
        chrome::{pane_block, pane_title},
        row::{self, PlaylistAreas, PlaylistRows, WindowFit, cursor_band, row_window},
        view::{LibraryStatus, PlaylistView},
    },
    primitive::list_chrome::{Scrollbar, paint_scrollbar, scroll_areas},
    theme::active_theme::ActiveTheme,
};

const EMPTY_PLAYLIST_TEXT: &str = "Empty playlist";

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaylistWidget<'a> {
    view: PlaylistView<'a>,
    active_theme: ActiveTheme<'a>,
}

impl<'a> PlaylistWidget<'a> {
    #[must_use]
    pub(crate) fn new(view: PlaylistView<'a>, active_theme: ActiveTheme<'a>) -> Self {
        Self { view, active_theme }
    }
}

impl PlaylistWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, pane: Rect) -> PlaylistAreas {
        if let Some(catalog_view) = self.view.catalog_view {
            return CatalogWidget::new(catalog_view, self.active_theme).areas(pane);
        }
        let body = pane_block(None, Color::Reset).inner(pane);
        let scroll_areas = scroll_areas(pane, body);
        let window = row_window(&WindowFit {
            selected: self.view.selected,
            playing_index: self.view.playing_index,
            playlist_len: self.view.playlist.tracks.len(),
            height: scroll_areas.content.height,
        });
        let selected_area =
            cursor_band(scroll_areas.rows, window, self.view.selected.get());
        PlaylistAreas {
            pane,
            banner: Rect::default(),
            scroll_areas,
            window,
            selected_area,
        }
    }

    pub(crate) fn paint(&self, areas: &PlaylistAreas, buffer: &mut Buffer) {
        if let Some(catalog_view) = self.view.catalog_view {
            CatalogWidget::new(catalog_view, self.active_theme).paint(areas, buffer);
            return;
        }
        let pane = areas.pane;
        if pane.width == 0 || pane.height == 0 {
            return;
        }
        pane_block(
            Some(pane_title(
                pane,
                self.view.status_line_view,
                &self.active_theme,
            )),
            self.active_theme.colors().muted_foreground,
        )
        .render(pane, buffer);
        if areas.scroll_areas.content.width == 0
            || areas.scroll_areas.content.height == 0
        {
            return;
        }
        paint_body(buffer, areas, *self);
    }
}

impl Widget for &PlaylistWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(&self.areas(area), buffer);
    }
}

fn paint_body(
    buffer: &mut Buffer,
    areas: &PlaylistAreas,
    playlist_widget: PlaylistWidget<'_>,
) {
    let inner = areas.scroll_areas.content;
    let view = playlist_widget.view;
    let theme = playlist_widget.active_theme;
    let colors = theme.colors();

    if view.playlist.tracks.is_empty() {
        let text: &str = match view.library_status {
            LibraryStatus::Loading => theme.scanning_label.as_str(),
            LibraryStatus::Ready => EMPTY_PLAYLIST_TEXT,
        };
        Paragraph::new(text)
            .style(Style::default().fg(colors.foreground))
            .render(inner, buffer);
        return;
    }

    let window = areas.window;

    row::paint_rows(
        buffer,
        PlaylistRows {
            view,
            theme,
            rows: areas.scroll_areas.rows,
            window,
            selected_area: areas.selected_area,
        },
    );

    paint_scrollbar(
        areas.scroll_areas.scrollbar,
        Scrollbar {
            total: window.playlist_len,
            offset: usize::from(window.offset),
            viewport: usize::from(areas.scroll_areas.scrollbar.height),
            thumb: colors.muted_foreground,
            groove: colors.muted_foreground,
        },
        buffer,
    );
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use kernel::{
        domain::{
            cursor::Cursor,
            favorites::Favorites,
            index::ViewIndex,
            model::{Model, ScanStatus},
            playlist::Playlist,
            startup::Shuffle,
            time::Moment,
            track::{Track, TrackParts},
        },
        message::{Message, PlaybackRequest},
        update::update,
    };
    use ratatui::style::Color;

    use crate::{
        playlist::{
            pane::PlaylistWidget,
            view::{LibraryStatus, PlaylistView},
        },
        primitive::canvas::tests::find_text,
        status_line::StatusLineView,
        test_support::{noir, rendered},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: format!("{title}.mp3").into(),
            duration: std::time::Duration::from_secs(120),
            tags: kernel::domain::track::Tags {
                title: Some(title.to_string()),
                ..kernel::domain::track::Tags::default()
            },
            audio_format: kernel::domain::track::AudioFormat::default(),
        }))
    }

    fn library(count: usize) -> Playlist {
        Playlist {
            tracks: (0..count)
                .map(|index| titled_track(&format!("song{index:02}")))
                .collect(),
            ..Playlist::default()
        }
    }

    fn status<'a>(
        playlist: &'a Playlist,
        queue: &[kernel::domain::track::TrackSource],
        theme: &'a Theme,
    ) -> StatusLineView<'a> {
        StatusLineView {
            shuffle: if playlist.play_order.is_shuffle() {
                Shuffle::On
            } else {
                Shuffle::Off
            },
            repeat_mode: playlist.repeat_mode,
            queue_len: queue.len(),
            selected: ViewIndex::new(0),
            playlist_len: playlist.tracks.len(),
            scan_status: ScanStatus::Idle,
            scanning_label: theme.scanning_label.as_str(),
            theme_name: theme.name.as_str(),
            remaining: None,
            servers: &[],
            catalog_name: &kernel::domain::catalog::CatalogName::Local,
        }
    }

    fn view<'a>(playlist: &'a Playlist, theme: &'a Theme) -> PlaylistView<'a> {
        PlaylistView {
            playlist,
            queue: &[],
            favorites: &EMPTY_FAVORITES,
            selected: ViewIndex::new(0),
            playing_index: None,
            library_status: LibraryStatus::Ready,
            catalog_view: None,
            status_line_view: status(playlist, &[], theme),
        }
    }

    fn queued(
        playlist: &Playlist,
        rows: &[usize],
    ) -> Vec<kernel::domain::track::TrackSource> {
        rows.iter()
            .map(|&row| playlist.tracks[row].source().clone())
            .collect()
    }

    static EMPTY_FAVORITES: std::sync::LazyLock<Favorites> =
        std::sync::LazyLock::new(Favorites::default);

    fn pane<'a>(playlist: &'a Playlist, theme: &'a Theme) -> PlaylistWidget<'a> {
        PlaylistWidget::new(
            view(playlist, theme),
            ActiveTheme::new(theme, ColorDepth::TrueColor),
        )
    }

    #[test]
    fn an_empty_playlist_shows_the_placeholder_and_a_full_pane_title() {
        let playlist = Playlist::default();
        let theme = noir();
        let widget = pane(&playlist, &theme);
        let text = rendered(60, 24, |frame| frame.render_widget(&widget, frame.area()))
            .to_string();
        assert!(text.contains("Empty playlist"), "got {text:?}");
        assert!(text.contains("Playlist"), "got {text:?}");
    }

    #[test]
    fn markers_sit_in_their_own_columns() {
        let mut playlist = library(3);
        playlist.cursor = Cursor::at(3, 1);
        let theme = noir();
        let queue = queued(&playlist, &[2]);
        let mut favorites = Favorites::default();
        if let Some(first) = playlist.tracks.first() {
            favorites.toggle(first.source().clone());
        }
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &favorites,
                selected: ViewIndex::new(0),
                playing_index: Some(ViewIndex::new(1)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: status(&playlist, &queue, &theme),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(60, 8, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn every_queued_row_ends_with_its_queue_number_chip() {
        let playlist = library(14);
        let theme = noir();
        let queue = queued(&playlist, &(1..13).collect::<Vec<_>>());
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(0),
                playing_index: Some(ViewIndex::new(0)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: status(&playlist, &queue, &theme),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(60, 14, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn a_narrow_pane_truncates_the_title_and_keeps_the_chip_room_for_the_title() {
        let playlist = library(3);
        let theme = noir();
        let queue = queued(&playlist, &[1, 2]);
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(0),
                playing_index: Some(ViewIndex::new(0)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: status(&playlist, &queue, &theme),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(24, 8, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn a_narrow_pane_truncates_the_title_and_keeps_the_chip_room_for_the_chip_alone() {
        let playlist = library(3);
        let theme = noir();
        let queue = queued(&playlist, &[1, 2]);
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(0),
                playing_index: Some(ViewIndex::new(0)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: status(&playlist, &queue, &theme),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(12, 8, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn a_long_title_truncates_so_the_chip_still_follows_it() {
        let playlist = Playlist {
            tracks: vec![titled_track(
                "a very long track title that will not fit inside this pane",
            )],
            cursor: Cursor::at(1, 0),
            ..Playlist::default()
        };
        let theme = noir();
        let queue = queued(&playlist, &[0]);
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(0),
                playing_index: None,
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: status(&playlist, &queue, &theme),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(40, 8, |frame| frame.render_widget(&widget, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn a_listed_library_names_its_files_while_the_status_line_counts_the_tagging() {
        let playlist = Playlist {
            tracks: ["song01.flac", "song02.mp3", "song03.mkv"]
                .iter()
                .map(|file| Arc::new(Track::listed(&Path::new("/music").join(file))))
                .collect(),
            ..Playlist::default()
        };
        let theme = noir();
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(0),
                playing_index: None,
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: StatusLineView {
                    scan_status: ScanStatus::Tagging { done: 1, total: 3 },
                    ..status(&playlist, &[], &theme)
                },
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let text = rendered(80, 8, |frame| frame.render_widget(&widget, frame.area()))
            .to_string();
        assert!(text.contains("tagging 1/3"), "got {text:?}");
    }

    #[test]
    fn an_over_long_title_truncates_inside_the_right_inset() {
        const SCROLLBAR_COLUMN: usize = 1;
        const PANE_BORDER: char = '┃';

        let playlist = Playlist {
            tracks: vec![titled_track(
                "Brooke Valentine; Da Brat; Lil Jon; Remy Ma; Miss B — Girlfight \
                 (Remix;Edited; feat. Lil Jon)",
            )],
            ..Playlist::default()
        };
        let theme = noir();
        let widget = pane(&playlist, &theme);
        let buffer =
            rendered(80, 24, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();

        let (_, y) =
            find_text(&buffer, "Brooke Valentine").expect("the row is painted");
        let row_chars: Vec<char> = (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        let cut = row_chars
            .iter()
            .position(|&character| character == '…')
            .expect("an over-long title truncates with an ellipsis");
        let tail: String = row_chars
            .iter()
            .skip(cut + 1)
            .take(SCROLLBAR_COLUMN + 1)
            .collect();
        assert_eq!(
            tail,
            format!("{}{PANE_BORDER}", " ".repeat(SCROLLBAR_COLUMN)),
            "the truncated title must stop short of the border by the scrollbar's own column"
        );
    }

    #[test]
    fn the_cursor_row_wears_the_band_and_the_playing_row_wears_the_marker() {
        let playlist = library(3);
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let colors = active_theme.colors();
        let highlight = colors.highlight;
        let selection_text = colors.selection_foreground;
        let selection_background = colors.selection_background;

        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(2),
                playing_index: Some(ViewIndex::new(1)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: StatusLineView {
                    selected: ViewIndex::new(2),
                    ..status(&playlist, &[], &theme)
                },
            },
            active_theme,
        );
        let buffer =
            rendered(60, 24, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();

        let (playing_x, playing_y) =
            find_text(&buffer, "▶ song01").expect("playing row visible");
        let playing = buffer[(playing_x, playing_y)].style();
        assert_eq!(playing.fg, Some(highlight));
        assert_ne!(
            playing.bg,
            Some(selection_background),
            "the playing row is marked, not filled — the fill belongs to the cursor"
        );

        let (cursor_x, cursor_y) =
            find_text(&buffer, "song02").expect("cursor row visible");
        let cursor = buffer[(cursor_x, cursor_y)].style();
        assert_eq!(cursor.fg, Some(selection_text));
        assert_eq!(cursor.bg, Some(selection_background));
    }

    #[test]
    fn the_painted_band_is_the_selected_area() {
        let playlist = library(40);
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let selection_background = active_theme.colors().selection_background;
        let widget = PlaylistWidget::new(
            PlaylistView {
                selected: ViewIndex::new(25),
                ..view(&playlist, &theme)
            },
            active_theme,
        );
        let pane = ratatui::layout::Rect::new(0, 0, 60, 12);
        let areas = widget.areas(pane);
        let band = areas
            .selected_area
            .expect("the selection is inside the window");
        let mut buffer = ratatui::buffer::Buffer::empty(pane);
        widget.paint(&areas, &mut buffer);
        let painted: Vec<(u16, u16)> = (0..pane.height)
            .flat_map(|y| (0..pane.width).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer[(x, y)].style().bg == Some(selection_background))
            .collect();
        let expected: Vec<(u16, u16)> =
            (band.x..band.x + band.width).map(|x| (x, band.y)).collect();
        assert_eq!(painted, expected);
    }

    #[test]
    fn the_selection_band_runs_the_full_width_of_the_row() {
        let playlist = library(3);
        let theme = noir();
        let selection_background: Color =
            ActiveTheme::new(&theme, ColorDepth::TrueColor)
                .colors()
                .selection_background;

        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(2),
                playing_index: Some(ViewIndex::new(0)),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: StatusLineView {
                    selected: ViewIndex::new(2),
                    ..status(&playlist, &[], &theme)
                },
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let buffer =
            rendered(60, 24, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();

        let (x, y) = find_text(&buffer, "song02").expect("cursor row visible");
        let banded: Vec<u16> = (0..buffer.area.width)
            .filter(|&column| {
                buffer[(column, y)].style().bg == Some(selection_background)
            })
            .collect();
        let first = banded.first().copied().expect("the band is painted");
        let last = banded.last().copied().expect("the band is painted");
        assert!(first < x, "the band starts before the row's first glyph");
        assert_eq!(
            banded.len(),
            usize::from(last - first + 1),
            "the band is one unbroken run of cells"
        );
    }

    #[test]
    fn playlist_scrollbar_handles_large_offset_without_overflow() {
        let playlist = Playlist {
            tracks: (0..10_000)
                .map(|index| {
                    Arc::new(Track::listed(Path::new(&format!(
                        "/m/song{index:05}.flac"
                    ))))
                })
                .collect(),
            cursor: Cursor::at(10_000, 9_999),
            ..Playlist::default()
        };
        let theme = noir();
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                selected: ViewIndex::new(9_999),
                playing_index: None,
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: StatusLineView {
                    selected: ViewIndex::new(9_999),
                    ..status(&playlist, &[], &theme)
                },
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let buffer =
            rendered(50, 30, |frame| frame.render_widget(&widget, frame.area()))
                .buffer()
                .clone();

        let down_arrow = (0..buffer.area.height).find_map(|y| {
            (0..buffer.area.width)
                .find(|&x| buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "▼"))
                .map(|x| (x, y))
        });
        let (x, y) = down_arrow.expect("scrollbar down arrow should render");
        assert_eq!(
            buffer.cell((x, y - 1)).map(ratatui::buffer::Cell::symbol),
            Some("█"),
            "handle should sit on the last track cell, right above the down arrow"
        );
    }

    #[test]
    fn shuffle_next_keeps_playing_row_visible_when_cursor_follows() {
        let tracks: Vec<Arc<Track>> = (0..40)
            .map(|index| titled_track(&format!("song{index:02}")))
            .collect();
        let mut model = Model {
            playlist: Playlist::from_tracks(tracks),
            ..Model::default()
        };
        update(
            &mut model,
            Message::Playback(PlaybackRequest::ToggleShuffle),
            Moment::default(),
        )
        .unwrap();
        let mut order: Vec<ViewIndex> = vec![ViewIndex::new(0), ViewIndex::new(39)];
        order.extend((1..39).map(ViewIndex::new));
        update(&mut model, Message::ShuffleRolled(order), Moment::default()).unwrap();
        update(
            &mut model,
            Message::Playback(PlaybackRequest::Next),
            Moment::default(),
        )
        .unwrap();

        let playing = model
            .playlist
            .playing_index()
            .expect("advance must land on a track");
        assert_eq!(
            playing.get(),
            39,
            "sanity: shuffle actually jumped to the far end"
        );
        model.workspace.browse.cursor =
            Cursor::at(model.playlist.tracks.len(), playing.get());

        let theme = noir();
        let widget = PlaylistWidget::new(
            PlaylistView {
                playlist: &model.playlist,
                queue: &model.queue,
                favorites: &model.favorites,
                selected: ViewIndex::new(model.workspace.browse.selected().get()),
                playing_index: model.playing_index(),
                library_status: LibraryStatus::Ready,
                catalog_view: None,
                status_line_view: StatusLineView {
                    selected: ViewIndex::new(model.workspace.browse.selected().get()),
                    ..status(&model.playlist, &model.queue, &theme)
                },
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let text = rendered(60, 28, |frame| frame.render_widget(&widget, frame.area()))
            .to_string();
        assert!(text.contains("▶ song39"), "got {text:?}");
    }
}
