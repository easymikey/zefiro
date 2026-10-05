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
            ModalScrollAreas,
            OverlayAreas,
            column_width,
            indented,
            leading_cells,
        },
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        glyphs,
        list_chrome::{Scrollbar, paint_scrollbar, scroll_offset},
        relative_time::relative_time,
        span::{line, text},
        text::truncate,
    },
    theme::{active_theme::ActiveTheme, colors::Colors},
};

#[derive(Debug)]
pub(crate) struct HistoryWidget<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) entries: &'a [HistoryEntry],
    pub(crate) now: Moment,
    pub(crate) selected: RowIndex,
    pub(crate) container: ModalContainer<'a>,
}

impl HistoryWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement(&self.labels()).areas(screen))
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::List(areas) = areas else {
            return;
        };
        let Canvas { area, buffer } = canvas;
        let labels = self.labels();
        self.placement(&labels).paint(
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
        self.paint_rows(
            LabeledRows {
                areas,
                labels: &labels,
            },
            buffer,
        );
    }

    fn labels(&self) -> Vec<String> {
        self.entries.iter().map(played_label).collect()
    }

    fn placement(&self, labels: &[String]) -> ModalPlacement<'_> {
        let theme = self.theme;
        let measures = HistoryMeasures::of(labels);
        ModalPlacement {
            container: self.container,
            border_title: modal_title(
                glyphs::history::TITLE_WORD,
                track_count_text(self.entries.len()),
                theme.colors(),
            ),
            modal_title: glyphs::history::TITLE_WORD,
            content_width: measures.natural_width(COLUMN_SPACING),
            content_rows: Cells(small_count_u16(self.entries.len())),
            hint: None,
            theme,
        }
    }

    fn paint_rows(&self, rows: LabeledRows<'_>, buffer: &mut Buffer) {
        let LabeledRows { areas, labels } = rows;
        let colors = self.theme.colors();
        let table_area = areas.rows;
        let lead = leading_cells(&areas).0;
        let total = self.entries.len();
        let height = usize::from(table_area.height);
        let columns = HistoryColumns::for_width(column_width(&areas), COLUMN_SPACING);
        let offset = scroll_offset(self.selected.get(), total, height);
        let table = Table::new(
            self.entries.iter().zip(labels).map(|(played, label)| {
                entry_row(
                    &EntryRow {
                        played,
                        label,
                        columns,
                        lead,
                    },
                    colors,
                    self.now,
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
        let mut table_rows = TableState::new()
            .with_offset(offset)
            .with_selected(Some(self.selected.get()));
        StatefulWidget::render(table, table_area, buffer, &mut table_rows);

        paint_scrollbar(
            areas.scrollbar,
            Scrollbar {
                total,
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

fn played_label(played: &HistoryEntry) -> String {
    played
        .artist
        .as_deref()
        .filter(|artist| !artist.is_empty())
        .map_or_else(
            || played.title.clone(),
            |artist| {
                format!(
                    "{artist}{}{}",
                    glyphs::history::LABEL_SEPARATOR,
                    played.title
                )
            },
        )
}

const WHEN_COLUMN_CELLS: Cells = Cells(8);

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistoryMeasures {
    label: Cells,
}

impl HistoryMeasures {
    fn of(labels: &[String]) -> Self {
        let widest = labels.iter().map(|label| label.width()).max().unwrap_or(0);
        Self {
            label: Cells(small_count_u16(widest)),
        }
    }

    fn natural_width(self, spacing: u16) -> Cells {
        if self.label == Cells(0) {
            let placeholder = glyphs::history::EMPTY_PLACEHOLDER.width();
            return Cells(small_count_u16(placeholder));
        }
        HistoryColumns {
            label: self.label,
            when: WHEN_COLUMN_CELLS,
            spacing,
        }
        .total()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistoryColumns {
    label: Cells,
    when: Cells,
    spacing: u16,
}

impl HistoryColumns {
    fn for_width(width: Cells, spacing: u16) -> Self {
        let when = WHEN_COLUMN_CELLS;
        Self {
            label: Cells(width.0.saturating_sub(when.0.saturating_add(spacing))),
            when,
            spacing,
        }
    }

    fn total(self) -> Cells {
        Cells(
            self.label
                .0
                .saturating_add(self.when.0)
                .saturating_add(self.spacing),
        )
    }

    fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label.0),
            Constraint::Length(self.when.0),
        ]
    }
}

#[derive(Debug, Clone, Copy)]
struct LabeledRows<'a> {
    areas: ModalScrollAreas,
    labels: &'a [String],
}

struct EntryRow<'a> {
    played: &'a HistoryEntry,
    label: &'a str,
    columns: HistoryColumns,
    lead: u16,
}

fn when_label(played: &HistoryEntry, now: Moment) -> String {
    relative_time(now, played.at)
}

fn entry_row(row: &EntryRow<'_>, colors: Colors<Color>, now: Moment) -> Row<'static> {
    let [label, when] = entry_cells(row, now);
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.text)),
        Line::from(when)
            .right_aligned()
            .style(Style::default().fg(colors.muted_foreground)),
    ])
}

fn entry_cells(row: &EntryRow<'_>, now: Moment) -> [String; 2] {
    let columns = row.columns;
    let cell = |value: &str, width: Cells| truncate(value, width.count()).into_owned();
    [
        indented(row.label, Cells(row.lead), columns.label),
        cell(&when_label(row.played, now), columns.when),
    ]
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{history::HistoryEntry, index::RowIndex, time::Moment};
    use ratatui::layout::Rect;

    use crate::{
        overlay::{history::HistoryWidget, modal::placement::ModalContainer},
        primitive::canvas::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn now() -> Moment {
        Moment::new(Duration::from_secs(1_700_000_000))
    }

    fn entry(path: &str, title: &str, artist: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            track: kernel::domain::track::TrackRef::Local(path.into()),
            title: title.to_string(),
            artist: artist.map(str::to_string),
            at: now(),
        }
    }

    fn scrolling_entries() -> Vec<HistoryEntry> {
        (0..40)
            .map(|index| {
                entry(
                    &format!("/m/{index:02}.flac"),
                    &format!("Song {index:02}"),
                    Some(&format!("Artist {index}")),
                )
            })
            .collect()
    }

    #[test]
    fn history_overlay_lists_its_entries() {
        let theme = noir();
        let entries = [
            entry("/m/a.flac", "Alpha", Some("Artist A")),
            entry("/m/b.flac", "Beta", None),
        ];
        let overlay = HistoryWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            now: now(),
            selected: RowIndex::new(1),
            container: ModalContainer::Modal(&[]),
        };
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn history_overlay_highlights_the_selected_row_and_aligns_its_label_column() {
        let theme = noir();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let entries = [
            entry("/m/a.flac", "Alpha", Some("Artist A")),
            entry("/m/b.flac", "Beta", None),
        ];
        let overlay = HistoryWidget {
            theme: active,
            entries: &entries,
            now: now(),
            selected: RowIndex::new(1),
            container: ModalContainer::Modal(&[]),
        };
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let selection_background = active.colors().selection_background;
        let (alpha_x, alpha_y) = find_text(&buffer, "Artist A — Alpha").unwrap();
        let (beta_x, beta_y) = find_text(&buffer, "Beta").unwrap();
        assert_eq!(
            buffer[(beta_x, beta_y)].style().bg,
            Some(selection_background)
        );
        assert_ne!(
            buffer[(alpha_x, alpha_y)].style().bg,
            Some(selection_background)
        );
        assert_eq!(alpha_x, beta_x);
    }

    #[test]
    fn history_overlay_with_a_scrollbar_keeps_its_time_column_clear_of_it() {
        let theme = noir();
        let entries = scrolling_entries();
        let overlay = HistoryWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            now: now(),
            selected: RowIndex::new(0),
            container: ModalContainer::Playlist(Rect::new(0, 0, 120, 40)),
        };
        insta::assert_snapshot!(
            rendered(120, 40, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn the_history_time_column_never_touches_the_scrollbar() {
        let theme = noir();
        let entries = scrolling_entries();
        let overlay = HistoryWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            now: now(),
            selected: RowIndex::new(0),
            container: ModalContainer::Playlist(Rect::new(0, 0, 120, 40)),
        };
        let buffer =
            rendered(120, 40, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let when = "just now";
        let (x, y) = find_text(&buffer, when).unwrap();
        let after = x + u16::try_from(when.chars().count()).unwrap();
        assert_eq!(buffer[(after, y)].symbol(), " ");
        assert_ne!(buffer[(after + 1, y)].symbol(), " ");
    }

    #[test]
    fn history_overlay_shows_a_placeholder_when_empty() {
        let theme = noir();
        let entries: [HistoryEntry; 0] = [];
        let overlay = HistoryWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            now: now(),
            selected: RowIndex::new(0),
            container: ModalContainer::Modal(&[]),
        };
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn history_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let entries: [HistoryEntry; 0] = [];
        let overlay = HistoryWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            now: now(),
            selected: RowIndex::new(0),
            container: ModalContainer::Modal(&[]),
        };
        assert_eq!(
            rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .area
                .height,
            3
        );
    }
}
