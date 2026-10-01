use std::sync::Arc;

use kernel::domain::{CursorOver, SearchQuery, Track};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{ModalRowColors, indented},
    primitive::{
        glyphs,
        list_chrome::scroll_offset,
        span::{line, text},
        text::truncate,
        track_row::Selected,
    },
};

pub(crate) struct SearchMatchList<'a> {
    pub(crate) area: Rect,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) search: &'a CursorOver<SearchQuery>,
    pub(crate) colors: ModalRowColors,
    pub(crate) lead: u16,
    pub(crate) scroll_padding: usize,
}

fn match_count_line(count: usize, dim: Color) -> Option<Line<'static>> {
    (count == 0).then(|| line([text(glyphs::search::NO_MATCHES).fg(dim)]))
}

pub(crate) fn render_match_pane(list: &SearchMatchList<'_>, buffer: &mut Buffer) {
    let SearchMatchList {
        area,
        tracks,
        search,
        colors,
        ..
    } = *list;
    if let Some(line) = match_count_line(search.content.matches.len(), colors.dim) {
        Paragraph::new(line).render(area, buffer);
        return;
    }

    let items: Vec<ListItem<'static>> = search
        .content
        .matches
        .iter()
        .filter_map(|&track_index| {
            tracks.get(track_index).map(|track| {
                ListItem::new(Line::from(indented(
                    track.display(),
                    list.lead,
                    area.width,
                )))
            })
        })
        .collect();

    let widget = List::new(items)
        .scroll_padding(list.scroll_padding)
        .style(Style::default().fg(colors.text))
        .highlight_style(colors.highlight());
    let mut visible = ListState::default().with_selected(Some(search.selected()));
    StatefulWidget::render(widget, area, buffer, &mut visible);
}

struct MatchRow<'a> {
    title: &'a str,
    selected: Selected,
    row_width: usize,
}

pub(crate) fn render_match_rows(list: &SearchMatchList<'_>, buffer: &mut Buffer) {
    let SearchMatchList {
        area,
        tracks,
        search,
        colors,
        ..
    } = *list;
    if area.height == 0 {
        return;
    }
    if let Some(line) = match_count_line(search.content.matches.len(), colors.dim) {
        Paragraph::new(line).render(area, buffer);
        return;
    }

    let row_width = usize::from(area.width);
    let lines: Vec<Line<'_>> = search
        .content
        .matches
        .iter()
        .enumerate()
        .filter_map(|(row, &index)| {
            tracks.get(index).map(|track| {
                let selected = if row == search.selected() {
                    Selected::Yes
                } else {
                    Selected::No
                };
                let row_props = MatchRow {
                    title: track.display(),
                    selected,
                    row_width,
                };
                match_line(&row_props, colors)
            })
        })
        .collect();

    let height = usize::from(area.height);
    let offset = scroll_offset(search.selected(), lines.len(), height);
    let offset = u16::try_from(offset).unwrap_or(u16::MAX);
    Paragraph::new(lines)
        .scroll((offset, 0))
        .render(area, buffer);
}

fn match_line<'a>(hit: &MatchRow<'a>, colors: ModalRowColors) -> Line<'a> {
    let base_text = match hit.selected {
        Selected::Yes => colors.selected_text,
        Selected::No => colors.text,
    };
    let marker = match hit.selected {
        Selected::Yes => glyphs::search::SELECTED_MARKER,
        Selected::No => glyphs::search::UNSELECTED_MARKER,
    };

    let title_width = hit.row_width.saturating_sub(marker.width());
    let content = truncate(hit.title, title_width);

    let marker_piece = match hit.selected {
        Selected::Yes => text(marker).fg(base_text).bg(colors.selected_background),
        Selected::No => text(marker).fg(colors.dim),
    };
    let content_piece = match hit.selected {
        Selected::Yes => text(content).fg(base_text).bg(colors.selected_background),
        Selected::No => text(content).fg(base_text),
    };

    line([marker_piece, content_piece])
}
