use std::{iter::once, sync::Arc};

use kernel::domain::{
    cursor_over::CursorOver,
    index::RowIndex,
    overlay::{SearchQuery, ServerQuery},
    track::{CatalogRow, Track},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{HighlightSpacing, List, ListState, Paragraph, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        glyphs,
        list_chrome::scroll_offset,
        span::{line, text},
        track_row::Selected,
        truncate::{blanks, truncate, truncate_line},
    },
    theme::colors::Colors,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Query<'a> {
    Search(&'a CursorOver<SearchQuery>),
    ServerSearch(&'a CursorOver<ServerQuery>),
}

pub(crate) struct SearchMatchList<'a> {
    pub(crate) area: Rect,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) search_query: Query<'a>,
    pub(crate) colors: Colors<Color>,
    pub(crate) lead: u16,
}

fn no_matches_line(count: usize, dim: Color) -> Option<Line<'static>> {
    (count == 0).then(|| line([text(glyphs::search::NO_MATCHES).fg(dim)]))
}

fn visible_matches<'a>(
    list: &SearchMatchList<'a>,
    search_query: &'a CursorOver<SearchQuery>,
) -> impl Iterator<Item = (Selected, &'a Track)> {
    let tracks = list.tracks;
    let matches = &search_query.content.matches;
    let selected = usize::from(search_query.selected());
    let height = usize::from(list.area.height);
    let offset = scroll_offset(RowIndex::new(selected), matches.len(), height);
    matches
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .filter_map(move |(row, &index)| {
            let selection = if row == selected {
                Selected::Yes
            } else {
                Selected::No
            };
            tracks
                .get(usize::from(index))
                .map(|track| (selection, track.as_ref()))
        })
}

pub(crate) fn paint_match_pane(list: &SearchMatchList<'_>, buffer: &mut Buffer) {
    let SearchMatchList {
        area,
        search_query,
        colors,
        tracks: _,
        lead,
    } = *list;
    let search_query = match search_query {
        Query::Search(search_query) => search_query,
        Query::ServerSearch(server_query) => {
            paint_rows(list, server_query, buffer);
            return;
        }
    };
    if let Some(line) =
        no_matches_line(search_query.content.matches.len(), colors.muted_foreground)
    {
        Paragraph::new(line).render(area, buffer);
        return;
    }

    let budget = usize::from(area.width).saturating_sub(usize::from(lead));
    let lines: Vec<Line<'_>> = visible_matches(list, search_query)
        .map(|(selected, track)| {
            let row = line([
                text(blanks(usize::from(lead))),
                text(truncate(track.display(), budget)),
            ]);
            match selected {
                Selected::Yes => row.style(
                    Style::default()
                        .fg(colors.selection_foreground)
                        .bg(colors.selection_background),
                ),
                Selected::No => row,
            }
        })
        .collect();
    Paragraph::new(lines)
        .style(Style::default().fg(colors.foreground))
        .render(area, buffer);
}

struct MatchRow<'a> {
    title: &'a str,
    selected: Selected,
    row_width: usize,
}

pub(crate) fn paint_match_rows(list: &SearchMatchList<'_>, buffer: &mut Buffer) {
    let SearchMatchList {
        area,
        search_query,
        colors,
        tracks: _,
        lead: _,
    } = *list;
    if area.height == 0 {
        return;
    }
    let search_query = match search_query {
        Query::Search(search_query) => search_query,
        Query::ServerSearch(server_query) => {
            paint_rows(list, server_query, buffer);
            return;
        }
    };
    if let Some(line) =
        no_matches_line(search_query.content.matches.len(), colors.muted_foreground)
    {
        Paragraph::new(line).render(area, buffer);
        return;
    }

    let row_width = usize::from(area.width);
    let lines: Vec<Line<'_>> = visible_matches(list, search_query)
        .map(|(selected, track)| {
            let match_row = MatchRow {
                title: track.display(),
                selected,
                row_width,
            };
            match_line(&match_row, colors)
        })
        .collect();
    Paragraph::new(lines).render(area, buffer);
}

fn match_line<'a>(match_row: &MatchRow<'a>, colors: Colors<Color>) -> Line<'a> {
    let (marker, marker_style, content_style) = match match_row.selected {
        Selected::Yes => {
            let selected = Style::default()
                .fg(colors.selection_foreground)
                .bg(colors.selection_background);
            (glyphs::search::SELECTED_MARKER, selected, selected)
        }
        Selected::No => (
            glyphs::search::UNSELECTED_MARKER,
            Style::default().fg(colors.muted_foreground),
            Style::default().fg(colors.foreground),
        ),
    };

    let title_width = match_row.row_width.saturating_sub(marker.width());
    let content = truncate(match_row.title, title_width);

    line([
        text(marker).style(marker_style),
        text(content).style(content_style),
    ])
}

fn paint_rows(
    list: &SearchMatchList<'_>,
    server_query: &CursorOver<ServerQuery>,
    buffer: &mut Buffer,
) {
    let SearchMatchList {
        area,
        colors,
        lead,
        tracks: _,
        search_query: _,
    } = *list;
    let catalog_rows = &server_query.content.catalog_rows;
    if let Some(placeholder) = placeholder(&server_query.content) {
        Paragraph::new(placeholder)
            .style(Style::default().fg(colors.muted_foreground))
            .render(area, buffer);
        return;
    }
    let padding = blanks(usize::from(lead));
    let title_style = Style::default()
        .fg(colors.accent)
        .add_modifier(Modifier::BOLD);
    let row_width = usize::from(area.width);
    let lines: Vec<(Option<usize>, Line<'_>)> = catalog_rows
        .iter()
        .enumerate()
        .flat_map(|(index, catalog_row)| {
            catalog_rows
                .get(..=index)
                .and_then(group)
                .map(|label| {
                    (
                        None,
                        line([text(&*padding), text(label).style(title_style)]),
                    )
                })
                .into_iter()
                .chain(once((
                    Some(index),
                    truncate_line(row(catalog_row, &padding), row_width),
                )))
        })
        .collect();
    let selected = usize::from(server_query.selected());
    let cursor_row = lines.iter().position(|(index, _)| *index == Some(selected));
    let mut state = ListState::default().with_selected(cursor_row);
    StatefulWidget::render(
        List::new(lines.into_iter().map(|(_, line)| line))
            .style(Style::default().fg(colors.foreground))
            .highlight_spacing(HighlightSpacing::Never)
            .highlight_style(
                Style::default()
                    .fg(colors.selection_foreground)
                    .bg(colors.selection_background),
            ),
        area,
        buffer,
        &mut state,
    );
}

fn placeholder(server_query: &ServerQuery) -> Option<&'static str> {
    let ServerQuery {
        server_name: _,
        input,
        catalog_rows,
        revision,
    } = server_query;
    match revision {
        Some(_) => Some("searching…"),
        None if catalog_rows.is_empty() && !input.is_empty() => {
            Some(glyphs::search::NO_MATCHES)
        }
        None => None,
    }
}

fn group(catalog_rows: &[CatalogRow]) -> Option<&'static str> {
    match catalog_rows {
        []
        | [.., CatalogRow::Album(_), CatalogRow::Album(_)]
        | [.., CatalogRow::Track(_), CatalogRow::Track(_)] => None,
        [.., CatalogRow::Album(_)] => Some("Albums"),
        [.., CatalogRow::Track(_)] => Some("Tracks"),
    }
}

fn row<'a>(catalog_row: &'a CatalogRow, padding: &'a str) -> Line<'a> {
    match catalog_row {
        CatalogRow::Album(server_album) => line([
            text(padding),
            text(&*server_album.artist),
            text(glyphs::search::ARTIST_SEPARATOR),
            text(&*server_album.title),
        ]),
        CatalogRow::Track(track) => line([text(padding), text(track.display())]),
    }
}
