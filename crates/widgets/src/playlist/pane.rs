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
        row::{
            self,
            PlaylistAreas,
            PlaylistRowsWidget,
            WindowFit,
            cursor_band,
            row_window,
        },
        view::{LibraryStatus, PlaylistView},
    },
    primitive::{
        list_chrome::{Scrollbar, paint_scrollbar, scroll_areas},
        span::{line, text},
    },
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
            playlist_len: self.view.rows.len(),
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

    if view.rows.is_empty() {
        let label = match view.library_status {
            LibraryStatus::Loading => line(
                theme
                    .spinner
                    .mark(&colors)
                    .into_iter()
                    .chain([text(theme.theme.scanning_label.as_str())]),
            ),
            LibraryStatus::Ready => line([text(EMPTY_PLAYLIST_TEXT)]),
        };
        Paragraph::new(label)
            .style(Style::default().fg(colors.foreground))
            .render(inner, buffer);
        return;
    }

    let window = areas.window;

    row::paint_rows(
        buffer,
        PlaylistRowsWidget {
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

    use kernel::domain::{
        appearance::Rgb,
        cursor::Cursor,
        favorites::Favorites,
        index::ViewIndex,
        model::ScanStatus,
        playlist::{Playlist, PlaylistRows},
        startup::Shuffle,
        track::{Track, TrackParts},
    };

    use crate::{
        playlist::{
            pane::PlaylistWidget,
            view::{LibraryStatus, PlaylistView},
        },
        primitive::canvas::tests::find_text,
        status_line::StatusLineView,
        test_support::{noir, rendered},
        theme::{Theme, active_theme::ActiveTheme, colors::Colors, rgb::ColorDepth},
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
            spinner: crate::primitive::spinner::Spinner::default(),
            theme_name: theme.name.as_str(),
            remaining: None,
            servers: &[],
            catalog_name: &kernel::domain::catalog::CatalogName::Local,
            playlist_source: &kernel::domain::playlist::PlaylistSource::Named,
        }
    }

    fn view<'a>(playlist: &'a Playlist, theme: &'a Theme) -> PlaylistView<'a> {
        PlaylistView {
            rows: PlaylistRows::Tracks(&playlist.tracks),
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
    fn a_pane_with_no_room_inside_its_border_paints_only_the_border() {
        let theme = noir();
        let full_playlist = library(50);
        let empty_playlist = Playlist::default();
        let painted = rendered(2, 10, |frame| {
            frame.render_widget(&pane(&full_playlist, &theme), frame.area());
        });
        let border = rendered(2, 10, |frame| {
            frame.render_widget(&pane(&empty_playlist, &theme), frame.area());
        });
        assert_eq!(painted.buffer(), border.buffer());
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
    fn the_cursor_row_wears_the_band_and_the_playing_row_wears_the_marker() {
        let playlist = library(3);
        let theme = Theme {
            colors: Colors {
                selection_foreground: Rgb([0xff, 0xff, 0xff]),
                ..noir().colors
            },
            ..noir()
        };
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let colors = active_theme.colors();
        let highlight = colors.highlight;
        let selection_text = colors.selection_foreground;
        let selection_background = colors.selection_background;

        let widget = PlaylistWidget::new(
            PlaylistView {
                rows: PlaylistRows::Tracks(&playlist.tracks),
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

        let (other_x, other_y) =
            find_text(&buffer, "song00").expect("first row visible");
        assert_eq!(
            buffer[(other_x, other_y)].style().fg,
            Some(colors.foreground)
        );
    }

    #[test]
    fn a_playing_row_just_below_the_window_highlights_no_row() {
        let playlist = library(40);
        let theme = noir();
        let area = ratatui::layout::Rect::new(0, 0, 60, 12);
        let end = pane(&playlist, &theme).areas(area).window.end;
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let highlight = active_theme.colors().highlight;
        let widget = PlaylistWidget::new(
            PlaylistView {
                playing_index: Some(ViewIndex::new(end)),
                ..view(&playlist, &theme)
            },
            active_theme,
        );
        let areas = widget.areas(area);
        assert_eq!(areas.window.end, end);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        widget.paint(&areas, &mut buffer);
        let (last_x, last_y) = find_text(&buffer, &format!("song{:02}", end - 1))
            .expect("the last row of the window is painted");
        assert_ne!(buffer[(last_x, last_y)].style().fg, Some(highlight));
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
        let (text_x, _) =
            find_text(&buffer, "song25").expect("the selected row is painted");
        assert!(
            band.x < text_x,
            "the band starts before the row's first glyph"
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
                rows: PlaylistRows::Tracks(&playlist.tracks),
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
}
