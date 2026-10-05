use kernel::domain::{
    geometry::Cells,
    index::ViewIndex,
    track::{Track, TrackRef},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{HighlightSpacing, List, ListState, StatefulWidget},
};

use crate::{
    pixels::numeric::small_count_u16,
    playlist::view::PlaylistView,
    primitive::{
        list_chrome::scroll_offset,
        marker::{FAVORITE_COLUMNS, Favorite, QueuePosition},
        track_row::{self, Playing, Selected, TrackRow},
    },
    theme::active_theme::ActiveTheme,
};

pub(crate) struct RowWindow {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) offset: u16,
    pub(crate) total: usize,
}

pub(crate) struct WindowFit<'a> {
    pub(crate) view: PlaylistView<'a>,
    pub(crate) height: u16,
}

pub(crate) fn visible_rows(fit: &WindowFit<'_>) -> RowWindow {
    let &WindowFit { view, height } = fit;
    let selected_line = if view.playlist.tracks.is_empty() {
        0
    } else {
        view.browse_selected
    };
    let total = view.playlist.tracks.len();
    let scrolled =
        small_count_u16(scroll_offset(selected_line, total, usize::from(height)));

    let height = usize::from(height);
    let window = usize::from(scrolled)..usize::from(scrolled) + height;
    let offset = view
        .playing
        .map(ViewIndex::get)
        .filter(|playing_line| height > 0 && !window.contains(playing_line))
        .map(|playing_line| {
            let max_offset = small_count_u16(total.saturating_sub(height));
            if playing_line < window.start {
                small_count_u16(playing_line)
            } else {
                small_count_u16((playing_line + 1).saturating_sub(height))
            }
            .min(max_offset)
        })
        .filter(|&candidate| {
            (usize::from(candidate)..usize::from(candidate) + height)
                .contains(&selected_line)
        })
        .unwrap_or(scrolled);

    let start = usize::from(offset);
    let end = (start + height).min(total);
    RowWindow {
        start,
        end,
        offset,
        total,
    }
}

fn queue_position(queue: &[TrackRef], source: &TrackRef) -> Option<QueuePosition> {
    queue
        .iter()
        .position(|queued| queued == source)
        .map(|index| QueuePosition::new(index + 1))
}

#[derive(Clone, Copy)]
pub(crate) struct PlaylistRows<'a> {
    pub(crate) view: PlaylistView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) rows: Rect,
    pub(crate) window: &'a RowWindow,
}

struct PlaylistRowParts<'a> {
    view: PlaylistView<'a>,
    row_width: Cells,
    theme: ActiveTheme<'a>,
}

fn build_line<'a>(
    context: &PlaylistRowParts<'_>,
    index: usize,
    track: &'a Track,
) -> ratatui::text::Line<'a> {
    let view = context.view;
    let selected = if index == view.browse_selected {
        Selected::Yes
    } else {
        Selected::No
    };
    let favorite = if view.favorites.is_favorite(track.source()) {
        Favorite::Yes
    } else {
        Favorite::No
    };
    let playing_index = view.playing.map(ViewIndex::get);
    let playing = if playing_index == Some(index) {
        Playing::Yes
    } else {
        Playing::No
    };
    let row_view = TrackRow {
        title: track.display(),
        selected,
        favorite,
        playing,
        queued: queue_position(view.queue, track.source())
            .filter(|_| playing_index != Some(index)),
        row_width: context.row_width,
    };
    track_row::track_row_line(&row_view, &context.theme)
}

pub(crate) fn paint_rows(buffer: &mut Buffer, playlist_rows: PlaylistRows<'_>) {
    let PlaylistRows {
        view,
        theme,
        rows,
        window,
    } = playlist_rows;
    let colors = theme.colors();
    let context = PlaylistRowParts {
        view,
        row_width: Cells(rows.width),
        theme,
    };

    let start = window.start;
    let end = window.end;
    let lines: Vec<ratatui::text::Line<'_>> = view
        .playlist
        .tracks
        .get(start..end)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(relative_index, track)| {
            build_line(&context, start + relative_index, track)
        })
        .collect();

    let list = List::new(lines)
        .highlight_spacing(HighlightSpacing::Never)
        .highlight_style(Style::default().fg(colors.highlight));
    let mut playing_row = ListState::default().with_selected(
        view.playing
            .map(ViewIndex::get)
            .and_then(|index| index.checked_sub(start))
            .filter(|&relative| relative < end.saturating_sub(start)),
    );
    StatefulWidget::render(list, rows, buffer, &mut playing_row);

    if let Some(band) = cursor_band(rows, window, view.browse_selected) {
        buffer.set_style(band, Style::default().bg(colors.selection_background));
    }
}

pub(crate) fn cursor_band(
    band: Rect,
    window: &RowWindow,
    cursor_index: usize,
) -> Option<Rect> {
    if cursor_index < window.start || cursor_index >= window.end {
        return None;
    }
    let offset = u16::try_from(cursor_index.checked_sub(window.start)?).ok()?;
    (offset < band.height).then_some(Rect {
        x: band.x,
        y: band.y.saturating_add(offset),
        width: band.width,
        height: 1,
    })
}

#[must_use]
pub fn favorite_cell(row: Rect) -> Rect {
    let width = FAVORITE_COLUMNS.min(row.width);
    Rect {
        x: row.x,
        y: row.y,
        width,
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use kernel::domain::{favorites::Favorites, playlist::Playlist};

    use crate::{
        playlist::{
            row::{WindowFit, visible_rows},
            view::{LibraryLoad, PlaylistView},
        },
        status_line::StatusLineView,
    };

    fn library(count: usize) -> Playlist {
        Playlist {
            tracks: (0..count)
                .map(|index| {
                    Arc::new(kernel::domain::track::Track::listed(Path::new(&format!(
                        "song{index:02}.mp3"
                    ))))
                })
                .collect(),
            ..Playlist::default()
        }
    }

    fn view<'a>(
        playlist: &'a Playlist,
        favorites: &'a Favorites,
        browse_selected: usize,
    ) -> PlaylistView<'a> {
        PlaylistView {
            playlist,
            queue: &[],
            favorites,
            browse_selected,
            playing: None,
            library_loading: LibraryLoad::Ready,
            status: StatusLineView {
                shuffle: kernel::domain::startup::Shuffle::Disabled,
                repeat_mode: kernel::domain::playlist::RepeatMode::Off,
                queue_len: 0,
                position: kernel::domain::index::ViewIndex::new(browse_selected),
                total: playlist.tracks.len(),
                scan_status: kernel::domain::model::ScanStatus::Idle,
                scanning_label: "Scanning…",
                theme_name: "noir",
                sleep_left: None,
            },
        }
    }

    #[test]
    fn the_window_starts_at_zero_while_the_selection_fits_on_screen() {
        let playlist = library(10);
        let favorites = Favorites::default();
        let window = visible_rows(&WindowFit {
            view: view(&playlist, &favorites, 2),
            height: 5,
        });
        assert_eq!((window.start, window.end), (0, 5));
    }

    #[test]
    fn the_window_follows_the_selection_past_the_first_screen() {
        let playlist = library(40);
        let favorites = Favorites::default();
        let window = visible_rows(&WindowFit {
            view: view(&playlist, &favorites, 35),
            height: 10,
        });
        assert!(window.start > 0, "the window must scroll to reach 35");
        assert!(window.end - window.start <= 10);
        assert!((window.start..window.end).contains(&35));
    }

    #[test]
    fn the_window_shifts_to_keep_the_playing_row_visible_when_the_cursor_still_fits() {
        let playlist = library(40);
        let favorites = Favorites::default();
        let window = visible_rows(&WindowFit {
            view: PlaylistView {
                playing: Some(kernel::domain::index::ViewIndex::new(32)),
                ..view(&playlist, &favorites, 30)
            },
            height: 10,
        });
        assert!((window.start..window.end).contains(&32));
        assert!((window.start..window.end).contains(&30));
    }
}
