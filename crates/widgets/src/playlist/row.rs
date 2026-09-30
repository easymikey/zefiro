use kernel::domain::{PlaylistIndex, Track};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{HighlightSpacing, List, ListState, StatefulWidget},
};

use crate::{
    Playing,
    overlay::modal::ModalMetrics,
    playlist::{
        chrome::pane_block,
        pane::{PlaylistPane, PlaylistView},
    },
    primitive::{
        glyphs::PlaylistGlyphs,
        list_chrome::{row_band, scroll_offset, scrollbar_column},
        marker::{Favorite, MarkerColumns, QueuePosition},
        track_row::{self, RowColors, Selected, TrackRowView},
    },
};

pub(crate) struct VisibleRows {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) offset: u16,
    pub(crate) total: usize,
}

pub(crate) struct WindowFit<'a> {
    pub(crate) view: PlaylistView<'a>,
    pub(crate) playing_index: Option<usize>,
    pub(crate) height: u16,
}

pub(crate) fn visible_rows(input: &WindowFit<'_>) -> VisibleRows {
    let &WindowFit {
        view,
        playing_index,
        height,
    } = input;
    let selected_line = if view.playlist.tracks.is_empty() {
        0
    } else {
        view.browse_selected
    };
    let total = view.playlist.tracks.len();
    let mut offset =
        u16::try_from(scroll_offset(selected_line, total, usize::from(height)))
            .unwrap_or(u16::MAX);

    let height = usize::from(height);
    let window = usize::from(offset)..usize::from(offset) + height;
    if let Some(playing_line) = playing_index.filter(|_| height > 0)
        && !window.contains(&playing_line)
    {
        let max_offset =
            u16::try_from(total.saturating_sub(height)).unwrap_or(u16::MAX);
        let candidate = if playing_line < window.start {
            u16::try_from(playing_line).unwrap_or(u16::MAX)
        } else {
            u16::try_from((playing_line + 1).saturating_sub(height)).unwrap_or(u16::MAX)
        }
        .min(max_offset);
        let candidate_window = usize::from(candidate)..usize::from(candidate) + height;
        if candidate_window.contains(&selected_line) {
            offset = candidate;
        }
    }

    let start = usize::from(offset);
    let end = (start + height).min(total);
    VisibleRows {
        start,
        end,
        offset,
        total,
    }
}

fn queue_position(queue: &[PlaylistIndex], row: usize) -> Option<QueuePosition> {
    queue
        .iter()
        .position(|&queued| queued.get() == row)
        .map(|index| QueuePosition::new(index + 1))
}

#[derive(Clone, Copy)]
pub(crate) struct PlaylistRows<'a> {
    pub(crate) pane: PlaylistPane<'a>,
    pub(crate) rows: Rect,
    pub(crate) playing_index: Option<usize>,
    pub(crate) window: &'a VisibleRows,
}

struct PlaylistRowParts<'a> {
    view: PlaylistView<'a>,
    playing_index: Option<usize>,
    row_width: usize,
    colors: RowColors,
}

fn build_line(
    context: &PlaylistRowParts<'_>,
    index: usize,
    track: &Track,
) -> ratatui::text::Line<'static> {
    let view = context.view;
    let selected = if index == view.browse_selected {
        Selected::Yes
    } else {
        Selected::No
    };
    let favorite = if view.favorites.is_favorite(track.path()) {
        Favorite::Yes
    } else {
        Favorite::No
    };
    let playing = if context.playing_index == Some(index) {
        Playing::Yes
    } else {
        Playing::No
    };
    let row_view = TrackRowView {
        title: track.display(),
        selected,
        favorite,
        playing,
        queued: queue_position(view.queue, index)
            .filter(|_| context.playing_index != Some(index)),
        columns: MarkerColumns::default(),
        glyphs: PlaylistGlyphs::default(),
        row_width: context.row_width,
    };
    track_row::track_row_line(&row_view, context.colors)
}

pub(crate) fn paint_rows(buffer: &mut Buffer, input: PlaylistRows<'_>) {
    let PlaylistRows {
        pane,
        rows,
        playing_index,
        window,
    } = input;
    let view = pane.view;
    let theme = pane.theme;
    let context = PlaylistRowParts {
        view,
        playing_index,
        row_width: usize::from(rows.width),
        colors: RowColors {
            text: theme.text(),
            selection_text: theme.selection_foreground(),
            favorite: theme.favorite(),
            queue: theme.highlight(),
        },
    };

    let start = window.start;
    let end = window.end;
    let lines: Vec<ratatui::text::Line<'static>> = view
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
        .highlight_style(Style::default().fg(theme.highlight()));
    let mut playing_row = ListState::default().with_selected(
        playing_index
            .and_then(|index| index.checked_sub(start))
            .filter(|&relative| relative < end.saturating_sub(start)),
    );
    StatefulWidget::render(list, rows, buffer, &mut playing_row);

    if let Some(band) = cursor_band(rows, window, view.browse_selected) {
        buffer.set_style(band, Style::default().bg(theme.selection_background()));
    }
}

fn cursor_band(band: Rect, window: &VisibleRows, cursor_index: usize) -> Option<Rect> {
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
pub(crate) fn cursor_row(area: Rect, view: PlaylistView<'_>) -> Option<Rect> {
    if area.width == 0 || area.height == 0 || view.playlist.tracks.is_empty() {
        return None;
    }
    let inner = pane_block(None, ratatui::style::Color::Reset).inner(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    let scrollbar_inset = ModalMetrics::default().scrollbar_inset;
    let band = row_band(area, inner, scrollbar_column(area, inner, scrollbar_inset));
    let window = visible_rows(&WindowFit {
        view,
        playing_index: view.playing.map(PlaylistIndex::get),
        height: inner.height,
    });
    if view.browse_selected >= window.end {
        return None;
    }
    let offset = u16::try_from(view.browse_selected.checked_sub(window.start)?).ok()?;
    Some(Rect {
        x: band.x,
        y: band.y.checked_add(offset)?,
        width: band.width,
        height: 1,
    })
}

#[must_use]
pub fn favorite_cell(row: Rect) -> Rect {
    let width = MarkerColumns::default().favorite.min(row.width);
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

    use kernel::{domain::Favorites, playlist::Playlist};

    use crate::playlist::{
        pane::{LibraryLoad, PlaylistView},
        row::{WindowFit, visible_rows},
    };

    fn library(count: usize) -> Playlist {
        Playlist {
            tracks: (0..count)
                .map(|index| {
                    Arc::new(kernel::domain::Track::listed(Path::new(&format!(
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
            scan: kernel::domain::ScanStatus::Idle,
            sleep_left: None,
        }
    }

    #[test]
    fn the_window_starts_at_zero_while_the_selection_fits_on_screen() {
        let playlist = library(10);
        let favorites = Favorites::default();
        let window = visible_rows(&WindowFit {
            view: view(&playlist, &favorites, 2),
            playing_index: None,
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
            playing_index: None,
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
            view: view(&playlist, &favorites, 30),
            playing_index: Some(32),
            height: 10,
        });
        assert!((window.start..window.end).contains(&32));
        assert!((window.start..window.end).contains(&30));
    }
}
