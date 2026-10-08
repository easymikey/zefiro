use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use kernel::domain::{
    geometry::Cells,
    index::{RowIndex, ViewIndex},
    track::{Track, TrackSource},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{HighlightSpacing, List, ListState, StatefulWidget},
};

use crate::{
    pixels::numeric::small_count_u16,
    playlist::view::PlaylistView,
    primitive::{
        list_chrome::{ScrollAreas, scroll_offset},
        marker::{FAVORITE_COLUMNS, QueueNumber},
        track_row::{self, Playing, Selected, TrackRow},
    },
    theme::{active_theme::ActiveTheme, colors::Colors},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowWindow {
    pub start: usize,
    pub end: usize,
    pub offset: u16,
    pub playlist_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistAreas {
    pub pane: Rect,
    pub banner: Rect,
    pub scroll_areas: ScrollAreas,
    pub window: RowWindow,
    pub selected_area: Option<Rect>,
}

pub(crate) struct WindowFit {
    pub(crate) selected: ViewIndex,
    pub(crate) playing_index: Option<ViewIndex>,
    pub(crate) playlist_len: usize,
    pub(crate) height: u16,
}

pub(crate) fn row_window(fit: &WindowFit) -> RowWindow {
    let &WindowFit {
        selected,
        playing_index,
        playlist_len,
        height,
    } = fit;
    let selected_line = if playlist_len == 0 { 0 } else { selected.get() };
    let scrolled = small_count_u16(scroll_offset(
        RowIndex::new(selected_line),
        playlist_len,
        usize::from(height),
    ));

    let height = usize::from(height);
    let window = usize::from(scrolled)..usize::from(scrolled) + height;
    let offset = playing_index
        .map(ViewIndex::get)
        .filter(|playing_line| height > 0 && !window.contains(playing_line))
        .map(|playing_line| {
            let max_offset = small_count_u16(playlist_len.saturating_sub(height));
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
    let end = (start + height).min(playlist_len);
    RowWindow {
        start,
        end,
        offset,
        playlist_len,
    }
}

fn queue_numbers<'a>(
    queue: &[TrackSource],
    tracks: &'a [Arc<Track>],
) -> HashMap<&'a TrackSource, QueueNumber> {
    let track_sources: HashSet<&TrackSource> =
        tracks.iter().map(|track| track.source()).collect();
    let mut positions = HashMap::new();
    for (index, queued) in queue.iter().enumerate() {
        if let Some(&source) = track_sources.get(queued) {
            positions
                .entry(source)
                .or_insert_with(|| QueueNumber::new(index + 1));
        }
    }
    positions
}

#[derive(Clone, Copy)]
pub(crate) struct PlaylistRows<'a> {
    pub(crate) view: PlaylistView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) rows: Rect,
    pub(crate) window: RowWindow,
    pub(crate) selected_area: Option<Rect>,
}

struct PlaylistRowParts<'a> {
    view: PlaylistView<'a>,
    row_width: Cells,
    colors: Colors<Color>,
    positions: HashMap<&'a TrackSource, QueueNumber>,
}

fn build_line<'a>(
    playlist_row_parts: &PlaylistRowParts<'_>,
    index: ViewIndex,
    track: &'a Track,
) -> ratatui::text::Line<'a> {
    let view = playlist_row_parts.view;
    let selected = if index.get() == view.selected.get() {
        Selected::Yes
    } else {
        Selected::No
    };
    let favorite = view.favorites.favorite(track.source());
    let playing = if view.playing_index == Some(index) {
        Playing::Yes
    } else {
        Playing::No
    };
    let track_row = TrackRow {
        title: track.display(),
        selected,
        favorite,
        playing,
        queued_number: playlist_row_parts.positions.get(track.source()).copied(),
        row_width: playlist_row_parts.row_width,
    };
    track_row::track_row_line(&track_row, &playlist_row_parts.colors)
}

pub(crate) fn paint_rows(buffer: &mut Buffer, playlist_rows: PlaylistRows<'_>) {
    let PlaylistRows {
        view,
        theme,
        rows,
        window,
        selected_area,
    } = playlist_rows;
    let colors = theme.colors();
    let start = window.start;
    let end = window.end;
    let visible_tracks = view.playlist.tracks.get(start..end).unwrap_or(&[]);
    let playlist_row_parts = PlaylistRowParts {
        view,
        row_width: Cells(rows.width),
        colors,
        positions: queue_numbers(view.queue, visible_tracks),
    };

    let lines: Vec<ratatui::text::Line<'_>> = visible_tracks
        .iter()
        .enumerate()
        .map(|(relative_index, track)| {
            build_line(
                &playlist_row_parts,
                ViewIndex::new(start + relative_index),
                track,
            )
        })
        .collect();

    let list = List::new(lines)
        .highlight_spacing(HighlightSpacing::Never)
        .highlight_style(Style::default().fg(colors.highlight));
    let mut playing_row = ListState::default().with_selected(
        view.playing_index
            .map(ViewIndex::get)
            .and_then(|index| index.checked_sub(start))
            .filter(|&relative| relative < end.saturating_sub(start)),
    );
    StatefulWidget::render(list, rows, buffer, &mut playing_row);

    if let Some(band) = selected_area {
        buffer.set_style(band, Style::default().bg(colors.selection_background));
    }
}

pub(crate) fn cursor_band(
    band: Rect,
    window: RowWindow,
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
pub fn favorite_cell(area: Rect) -> Rect {
    let width = FAVORITE_COLUMNS.min(area.width);
    Rect {
        x: area.x,
        y: area.y,
        width,
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use kernel::domain::{index::ViewIndex, playlist::Playlist, track::TrackSource};

    use crate::{
        playlist::row::{WindowFit, queue_numbers, row_window},
        primitive::marker::QueueNumber,
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

    #[test]
    fn a_queued_row_takes_the_first_position_the_whole_queue_scan_would_find() {
        let playlist = library(4);
        let source = |index: usize| playlist.tracks[index].source().clone();
        let queue = [source(3), source(1), source(3), source(0)];
        let visible_tracks = playlist.tracks.get(1..3).unwrap_or(&[]);

        let positions = queue_numbers(&queue, visible_tracks);

        let found: Vec<Option<usize>> = visible_tracks
            .iter()
            .map(|track| {
                queue
                    .iter()
                    .position(|queued| queued == track.source())
                    .map(|index| index + 1)
            })
            .collect();
        assert_eq!(found, vec![Some(2), None]);
        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions.get(visible_tracks[0].source()).copied(),
            Some(QueueNumber::new(2))
        );
        assert!(!positions.contains_key::<TrackSource>(&source(0)));
    }

    fn fit(
        selected_line: usize,
        playing_index: Option<usize>,
        playlist_len: usize,
    ) -> WindowFit {
        WindowFit {
            selected: ViewIndex::new(selected_line),
            playing_index: playing_index.map(ViewIndex::new),
            playlist_len,
            height: 10,
        }
    }

    #[test]
    fn the_window_starts_at_zero_while_the_selection_fits_on_screen() {
        let window = row_window(&WindowFit {
            height: 5,
            ..fit(2, None, 10)
        });
        assert_eq!((window.start, window.end), (0, 5));
    }

    #[test]
    fn the_window_follows_the_selection_past_the_first_screen() {
        let window = row_window(&fit(35, None, 40));
        assert!(window.start > 0, "the window must scroll to reach 35");
        assert!(window.end - window.start <= 10);
        assert!((window.start..window.end).contains(&35));
    }

    #[test]
    fn the_window_shifts_to_keep_the_playing_row_visible_when_the_cursor_still_fits() {
        let window = row_window(&fit(30, Some(32), 40));
        assert!((window.start..window.end).contains(&32));
        assert!((window.start..window.end).contains(&30));
    }
}
