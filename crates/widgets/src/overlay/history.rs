use std::borrow::Cow;

use kernel::domain::{
    geometry::Cells,
    history::HistoryEntry,
    index::RowIndex,
    time::Moment,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Paragraph, Row, StatefulWidget, Table, TableState, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{
        metrics::{COLUMN_SPACING, modal_title},
        placement::{
            ModalContainer,
            ModalPlacement,
            column_width,
            indented,
            leading_cells,
        },
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        glyphs,
        list_chrome::{ScrollAreas, Scrollbar, paint_scrollbar, scroll_offset},
        span::{line, text},
        time_text::relative_time_text,
        truncate::truncate_owned,
    },
    theme::{active_theme::ActiveTheme, colors::Colors},
};

#[derive(Debug)]
pub(crate) struct HistoryWidget<'a> {
    theme: ActiveTheme<'a>,
    entries: &'a [HistoryEntry],
    measures: &'a HistoryMeasures,
    now: Moment,
    selected: RowIndex,
    container: ModalContainer<'a>,
}

impl<'a> HistoryWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        entries: &'a [HistoryEntry],
        measures: &'a HistoryMeasures,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            entries,
            measures,
            now: Moment::default(),
            selected: RowIndex::new(0),
            container: ModalContainer::Floating(&[]),
        }
    }

    #[must_use]
    pub(crate) fn now(mut self, now: Moment) -> Self {
        self.now = now;
        self
    }

    #[must_use]
    pub(crate) fn selected(mut self, selected: RowIndex) -> Self {
        self.selected = selected;
        self
    }

    #[must_use]
    pub(crate) fn container(mut self, container: ModalContainer<'a>) -> Self {
        self.container = container;
        self
    }
}

impl HistoryWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ScrollAreas {
        self.placement().areas(screen)
    }

    pub(crate) fn paint(&self, areas: ScrollAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        self.placement().paint(
            areas,
            Canvas {
                area,
                buffer: &mut *buffer,
            },
        );
        if areas.content.width == 0 || areas.content.height == 0 {
            return;
        }
        if self.entries.is_empty() {
            let dim = self.theme.colors().muted_foreground;
            let placeholder = glyphs::history::EMPTY_PLACEHOLDER;
            Paragraph::new(line([text(placeholder).fg(dim)]))
                .render(areas.content, buffer);
            return;
        }
        self.paint_rows(areas, buffer);
    }

    fn placement(&self) -> ModalPlacement<'_> {
        let theme = self.theme;
        ModalPlacement {
            container: self.container,
            border_title: modal_title(
                glyphs::history::TITLE_WORD,
                &self.measures.detail,
                theme.colors(),
            ),
            modal_title: glyphs::history::TITLE_WORD,
            content_width: self.measures.natural_width(COLUMN_SPACING),
            content_rows: Cells(small_count_u16(self.entries.len())),
            theme,
        }
    }

    fn paint_rows(&self, areas: ScrollAreas, buffer: &mut Buffer) {
        let colors = self.theme.colors();
        let table_area = areas.rows;
        let lead = leading_cells(&areas).0;
        let entries_len = self.entries.len();
        let height = usize::from(table_area.height);
        let columns = HistoryColumns::for_width(column_width(&areas), COLUMN_SPACING);
        let offset = scroll_offset(self.selected, entries_len, height);
        let table = Table::new(
            self.entries
                .iter()
                .skip(offset)
                .take(height)
                .map(|history_entry| {
                    entry_row(
                        history_entry,
                        &RowStyle {
                            columns,
                            lead,
                            colors,
                            now: self.now,
                        },
                    )
                }),
            columns.constraints(),
        )
        .column_spacing(columns.spacing)
        .row_highlight_style(
            Style::default()
                .fg(colors.selection_foreground)
                .bg(colors.selection_background),
        );
        let mut table_state = TableState::new()
            .with_selected(Some(self.selected.get().saturating_sub(offset)));
        StatefulWidget::render(table, table_area, buffer, &mut table_state);

        paint_scrollbar(
            areas.scrollbar,
            Scrollbar {
                total: entries_len,
                offset,
                viewport: height,
                thumb: colors.muted_foreground,
                groove: colors.muted_foreground,
            },
            buffer,
        );
    }
}

impl Widget for &HistoryWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

fn track_count_text(tracks: usize) -> String {
    let noun = if tracks == 1 {
        glyphs::history::TRACK_SINGULAR
    } else {
        glyphs::history::TRACK_PLURAL
    };
    format!("{tracks} {noun}")
}

fn played_label(history_entry: &HistoryEntry) -> Cow<'_, str> {
    history_entry
        .artist
        .as_deref()
        .filter(|artist| !artist.is_empty())
        .map_or_else(
            || Cow::Borrowed(history_entry.title.as_str()),
            |artist| {
                Cow::Owned(format!(
                    "{artist}{}{}",
                    glyphs::history::LABEL_SEPARATOR,
                    history_entry.title
                ))
            },
        )
}

const WHEN_COLUMN_CELLS: Cells = Cells(8);

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryMeasures {
    label_width: Cells,
    detail: String,
}

impl HistoryMeasures {
    #[must_use]
    pub(crate) fn of(entries: &[HistoryEntry]) -> Self {
        let widest = entries
            .iter()
            .map(|history_entry| {
                let title = history_entry.title.width();
                history_entry
                    .artist
                    .as_deref()
                    .filter(|artist| !artist.is_empty())
                    .map_or(title, |artist| {
                        artist.width()
                            + glyphs::history::LABEL_SEPARATOR.width()
                            + title
                    })
            })
            .max()
            .unwrap_or(0);
        Self {
            label_width: Cells(small_count_u16(widest)),
            detail: track_count_text(entries.len()),
        }
    }

    fn natural_width(&self, spacing: u16) -> Cells {
        if self.label_width == Cells(0) {
            let placeholder = glyphs::history::EMPTY_PLACEHOLDER.width();
            return Cells(small_count_u16(placeholder));
        }
        HistoryColumns {
            label_width: self.label_width,
            when_width: WHEN_COLUMN_CELLS,
            spacing,
        }
        .total()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistoryColumns {
    label_width: Cells,
    when_width: Cells,
    spacing: u16,
}

impl HistoryColumns {
    fn for_width(width: Cells, spacing: u16) -> Self {
        let when = WHEN_COLUMN_CELLS;
        Self {
            label_width: Cells(width.0.saturating_sub(when.0.saturating_add(spacing))),
            when_width: when,
            spacing,
        }
    }

    fn total(self) -> Cells {
        Cells(
            self.label_width
                .0
                .saturating_add(self.when_width.0)
                .saturating_add(self.spacing),
        )
    }

    fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label_width.0),
            Constraint::Length(self.when_width.0),
        ]
    }
}

struct RowStyle {
    columns: HistoryColumns,
    lead: u16,
    colors: Colors<Color>,
    now: Moment,
}

fn entry_row(history_entry: &HistoryEntry, row_style: &RowStyle) -> Row<'static> {
    let RowStyle {
        columns,
        lead,
        colors,
        now,
    } = *row_style;
    let label = indented(
        &played_label(history_entry),
        Cells(lead),
        columns.label_width,
    );
    let when = truncate_owned(
        relative_time_text(now, history_entry.played_at),
        columns.when_width.count(),
    );
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.foreground)),
        Line::from(when)
            .right_aligned()
            .style(Style::default().fg(colors.muted_foreground)),
    ])
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{history::HistoryEntry, index::RowIndex, time::Moment};
    use ratatui::layout::Rect;

    use crate::{
        overlay::{
            history::{HistoryMeasures, HistoryWidget},
            modal::placement::ModalContainer,
        },
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn now() -> Moment {
        Moment::new(Duration::from_secs(1_700_000_000))
    }

    fn entry(path: &str, title: &str, artist: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            track_source: kernel::domain::track::TrackSource::Local(path.into()),
            title: title.to_string(),
            artist: artist.map(str::to_string),
            played_at: now(),
        }
    }

    #[test]
    fn history_overlay_lists_its_entries() {
        let theme = noir();
        let entries = [
            entry("/m/a.flac", "Alpha", Some("Artist A")),
            entry("/m/b.flac", "Beta", None),
        ];
        let measures = HistoryMeasures::of(&entries);
        let overlay = HistoryWidget::new(
            &entries,
            &measures,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .now(now())
        .selected(RowIndex::new(1))
        .container(ModalContainer::Floating(&[]));
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn history_overlay_of_two_hundred_entries_scrolls_to_the_selected_one() {
        let theme = noir();
        let entries: Vec<HistoryEntry> = (0..200)
            .map(|index| {
                entry(
                    &format!("/m/{index:03}.flac"),
                    &format!("Song {index:03}"),
                    Some(&format!("Artist {index}")),
                )
            })
            .collect();
        let measures = HistoryMeasures::of(&entries);
        let overlay = HistoryWidget::new(
            &entries,
            &measures,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .now(now())
        .selected(RowIndex::new(150))
        .container(ModalContainer::Playlist(Rect::new(0, 0, 120, 40)));
        insta::assert_snapshot!(
            rendered(120, 40, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn history_overlay_shows_a_placeholder_when_empty() {
        let theme = noir();
        let entries: [HistoryEntry; 0] = [];
        let measures = HistoryMeasures::of(&entries);
        let overlay = HistoryWidget::new(
            &entries,
            &measures,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .now(now())
        .selected(RowIndex::new(0))
        .container(ModalContainer::Floating(&[]));
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }
}
