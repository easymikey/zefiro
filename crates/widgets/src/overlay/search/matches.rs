use kernel::domain::{
    cursor_over::CursorOver,
    index::RowIndex,
    overlay::SearchQuery,
    playlist::PlaylistRows,
    track::Track,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        glyphs,
        list_chrome::scroll_offset,
        span::{line, text},
        track_row::Selected,
        truncate::{blanks, truncate},
    },
    theme::colors::Colors,
};

pub(crate) struct SearchMatchList<'a> {
    pub(crate) area: Rect,
    pub(crate) rows: PlaylistRows<'a>,
    pub(crate) search_query: &'a CursorOver<SearchQuery>,
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
    let rows = list.rows;
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
            rows.get(index).map(|track| (selection, track.as_ref()))
        })
}

pub(crate) fn paint_match_pane(list: &SearchMatchList<'_>, buffer: &mut Buffer) {
    let SearchMatchList {
        area,
        search_query,
        colors,
        rows: _,
        lead,
    } = *list;
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
        rows: _,
        lead: _,
    } = *list;
    if area.height == 0 {
        return;
    }
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
