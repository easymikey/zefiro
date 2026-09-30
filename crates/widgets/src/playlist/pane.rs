use std::time::Duration;

use kernel::{
    domain::{Favorites, PlaylistIndex, ScanStatus},
    playlist::Playlist,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::modal::ModalMetrics,
    playlist::{
        chrome::{pane_block, pane_title},
        row::{self, PlaylistRows, WindowFit, cursor_row, visible_rows},
    },
    primitive::list_chrome::{
        ScrollbarTrack,
        render_scrollbar,
        row_band,
        scrollbar_column,
    },
    theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryLoad {
    Loading,
    Ready,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaylistView<'a> {
    pub(crate) playlist: &'a Playlist,
    pub(crate) queue: &'a [PlaylistIndex],
    pub(crate) favorites: &'a Favorites,
    pub(crate) browse_selected: usize,
    pub(crate) playing: Option<PlaylistIndex>,
    pub(crate) library_loading: LibraryLoad,
    pub(crate) scan: ScanStatus,
    pub(crate) sleep_left: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaylistPane<'a> {
    pub(crate) view: PlaylistView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistAreas {
    pub pane: Rect,
    pub body: Rect,
    pub rows: Rect,
    pub scrollbar: Rect,
    pub selected: Option<Rect>,
}

impl PlaylistPane<'_> {
    #[must_use]
    pub(crate) fn areas(&self, pane: Rect) -> PlaylistAreas {
        let body = pane_block(None, Color::Reset).inner(pane);
        let scrollbar =
            scrollbar_column(pane, body, ModalMetrics::default().scrollbar_inset);
        PlaylistAreas {
            pane,
            body,
            rows: row_band(pane, body, scrollbar),
            scrollbar,
            selected: cursor_row(pane, self.view),
        }
    }

    pub(crate) fn render_in(&self, areas: &PlaylistAreas, buffer: &mut Buffer) {
        let pane = areas.pane;
        if pane.width == 0 || pane.height == 0 {
            return;
        }
        pane_block(
            Some(pane_title(pane, self.view, self.theme)),
            self.theme.border(),
        )
        .render(pane, buffer);
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        paint_body(buffer, areas, *self);
    }
}

impl Widget for &PlaylistPane<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(&self.areas(area), buffer);
    }
}

fn paint_body(buffer: &mut Buffer, areas: &PlaylistAreas, pane: PlaylistPane<'_>) {
    let inner = areas.body;
    let view = pane.view;
    let theme = pane.theme;
    let text_color: Color = theme.text();

    if view.playlist.tracks.is_empty() {
        let text: &str = match view.library_loading {
            LibraryLoad::Loading => theme.scanning_label.as_str(),
            LibraryLoad::Ready => "Empty playlist",
        };
        Paragraph::new(text)
            .style(Style::default().fg(text_color))
            .render(inner, buffer);
        return;
    }

    let playing_index = view.playing.map(PlaylistIndex::get);

    let window = visible_rows(&WindowFit {
        view,
        playing_index,
        height: inner.height,
    });

    row::paint_rows(
        buffer,
        PlaylistRows {
            pane,
            rows: areas.rows,
            playing_index,
            window: &window,
        },
    );

    render_scrollbar(
        areas.scrollbar,
        ScrollbarTrack {
            total: window.total,
            offset: usize::from(window.offset),
            viewport: usize::from(areas.scrollbar.height),
            thumb: theme.border(),
            track: theme.dim(),
        },
        buffer,
    );
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use kernel::{
        Message,
        Moment,
        PlaybackRequest,
        PlaylistRequest,
        domain::{Cursor, Favorites, Model, PlaylistIndex, ScanStatus, Track},
        playlist::Playlist,
        update::update,
    };
    use ratatui::style::Color;

    use crate::{
        playlist::pane::{LibraryLoad, PlaylistPane, PlaylistView},
        scene::fixtures::{find_text, noir, painted, painted_buffer},
        theme::{ActiveTheme, ColorDepth},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("{title}.mp3"))
                .duration(Duration::from_secs(120))
                .tags(kernel::domain::Tags {
                    title: Some(title.to_string()),
                    ..kernel::domain::Tags::default()
                })
                .audio_format(kernel::domain::AudioFormat::default())
                .build(),
        )
    }

    fn library(count: usize) -> Playlist {
        Playlist {
            tracks: (0..count)
                .map(|index| titled_track(&format!("song{index:02}")))
                .collect(),
            ..Playlist::default()
        }
    }

    fn view(playlist: &Playlist) -> PlaylistView<'_> {
        PlaylistView {
            playlist,
            queue: &[],
            favorites: &EMPTY_FAVORITES,
            browse_selected: 0,
            playing: None,
            library_loading: LibraryLoad::Ready,
            scan: ScanStatus::Idle,
            sleep_left: None,
        }
    }

    static EMPTY_FAVORITES: std::sync::LazyLock<Favorites> =
        std::sync::LazyLock::new(Favorites::default);

    fn pane<'a>(
        playlist: &'a Playlist,
        theme: &'a crate::theme::Theme,
    ) -> PlaylistPane<'a> {
        PlaylistPane {
            view: view(playlist),
            theme: ActiveTheme::new(theme, ColorDepth::TrueColor),
        }
    }

    #[test]
    fn an_empty_playlist_shows_the_placeholder_and_a_full_pane_title() {
        let playlist = Playlist::default();
        let theme = noir();
        let widget = pane(&playlist, &theme);
        let text = painted(&widget, 60, 24);
        assert!(text.contains("Empty playlist"), "got {text:?}");
        assert!(text.contains("Playlist"), "got {text:?}");
    }

    #[test]
    fn markers_sit_in_their_own_columns() {
        let mut playlist = library(3);
        playlist.cursor = Cursor::with_len(3).at(1);
        let theme = noir();
        let queue = [PlaylistIndex::new(2)];
        let mut favorites = Favorites::default();
        if let Some(first) = playlist.tracks.first() {
            favorites.toggle(first.path().to_path_buf());
        }
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &favorites,
                browse_selected: 0,
                playing: Some(PlaylistIndex::new(1)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        insta::assert_snapshot!(painted(&widget, 60, 8));
    }

    #[test]
    fn every_queued_row_ends_with_its_position_chip() {
        let playlist = library(14);
        let theme = noir();
        let queue: Vec<PlaylistIndex> = (1..13).map(PlaylistIndex::new).collect();
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                browse_selected: 0,
                playing: Some(PlaylistIndex::new(0)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        insta::assert_snapshot!(painted(&widget, 60, 14));
    }

    #[test]
    fn a_narrow_pane_truncates_the_title_and_keeps_the_chip_room_for_the_title() {
        let playlist = library(3);
        let theme = noir();
        let queue = [PlaylistIndex::new(1), PlaylistIndex::new(2)];
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                browse_selected: 0,
                playing: Some(PlaylistIndex::new(0)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        insta::assert_snapshot!(painted(&widget, 24, 8));
    }

    #[test]
    fn a_narrow_pane_truncates_the_title_and_keeps_the_chip_room_for_the_chip_alone() {
        let playlist = library(3);
        let theme = noir();
        let queue = [PlaylistIndex::new(1), PlaylistIndex::new(2)];
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                browse_selected: 0,
                playing: Some(PlaylistIndex::new(0)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        insta::assert_snapshot!(painted(&widget, 12, 8));
    }

    #[test]
    fn a_long_title_truncates_so_the_chip_still_follows_it() {
        let playlist = Playlist {
            tracks: vec![titled_track(
                "a very long track title that will not fit inside this pane",
            )],
            cursor: Cursor::with_len(1).at(0),
            ..Playlist::default()
        };
        let theme = noir();
        let queue = [PlaylistIndex::new(0)];
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &queue,
                favorites: &EMPTY_FAVORITES,
                browse_selected: 0,
                playing: None,
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        insta::assert_snapshot!(painted(&widget, 40, 8));
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
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                browse_selected: 0,
                playing: None,
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Tagging { done: 1, total: 3 },
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let text = painted(&widget, 80, 8);
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
        let buffer = painted_buffer(&widget, 80, 24);

        let (_, y) =
            find_text(&buffer, "Brooke Valentine").expect("the row is painted");
        let row: Vec<char> = (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        let cut = row
            .iter()
            .position(|&character| character == '…')
            .expect("an over-long title truncates with an ellipsis");
        let tail: String = row
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
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let highlight = active.highlight();
        let selection_text = active.selection_foreground();
        let selection_background = active.selection_background();

        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                browse_selected: 2,
                playing: Some(PlaylistIndex::new(1)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: active,
        };
        let buffer = painted_buffer(&widget, 60, 24);

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
    fn the_selection_band_runs_the_full_width_of_the_row() {
        let playlist = library(3);
        let theme = noir();
        let selection_background: Color =
            ActiveTheme::new(&theme, ColorDepth::TrueColor).selection_background();

        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                browse_selected: 2,
                playing: Some(PlaylistIndex::new(0)),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let buffer = painted_buffer(&widget, 60, 24);

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
            cursor: Cursor::with_len(10_000).at(9_999),
            ..Playlist::default()
        };
        let theme = noir();
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &playlist,
                queue: &[],
                favorites: &EMPTY_FAVORITES,
                browse_selected: 9_999,
                playing: None,
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let buffer = painted_buffer(&widget, 50, 30);

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
        let _ = update(
            &mut model,
            Message::Playback(PlaybackRequest::ToggleShuffle),
            Moment::default(),
        );
        let mut order: Vec<usize> = vec![0, 39];
        order.extend(1..39);
        let _ = update(
            &mut model,
            Message::Loaded(PlaylistRequest::ShuffleRolled(order)),
            Moment::default(),
        );
        let _ = update(
            &mut model,
            Message::Playback(PlaybackRequest::Next),
            Moment::default(),
        );

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
            Cursor::with_len(model.playlist.tracks.len()).at(playing.get());

        let theme = noir();
        let widget = PlaylistPane {
            view: PlaylistView {
                playlist: &model.playlist,
                queue: &model.queue,
                favorites: &model.favorites,
                browse_selected: model.workspace.browse.selected().get(),
                playing: model.playing_index(),
                library_loading: LibraryLoad::Ready,
                scan: ScanStatus::Idle,
                sleep_left: None,
            },
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let text = painted(&widget, 60, 28);
        assert!(text.contains("▶ song39"), "got {text:?}");
    }
}
