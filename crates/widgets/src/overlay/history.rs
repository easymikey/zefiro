use kernel::domain::HistoryEntry;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Style,
    text::Line,
    widgets::{Paragraph, Row, StatefulWidget, Table, TableState, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{
        ModalChrome,
        ModalMetrics,
        ModalPlacement,
        ModalRowColors,
        ModalScrollAreas,
        OverlayAreas,
        OverlayContainer,
        column_width,
        lead_cells,
        led,
        modal_title,
    },
    primitive::{
        canvas::Canvas,
        glyphs::{HistoryGlyphs, TruncateGlyphs},
        inset::Inset,
        list_chrome::{ScrollbarTrack, render_scrollbar, scroll_offset},
        relative_time::relative_time,
        span::{row, text},
        text::truncate_to_width,
    },
    theme::ActiveTheme,
};

#[derive(Debug)]
pub(crate) struct HistoryOverlay<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) entries: &'a [HistoryEntry],
    pub(crate) selected: usize,
    pub(crate) now_unix: u64,
    pub(crate) container: OverlayContainer<'a>,
}

impl HistoryOverlay<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::List(self.placement().areas(screen))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::List(areas) = areas else {
            return;
        };
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
            let dim = self.theme.dim();
            let placeholder = HistoryGlyphs::default().empty_placeholder;
            Paragraph::new(row([text(placeholder).fg(dim)]))
                .render(areas.content, buffer);
            return;
        }
        self.render_rows(areas, buffer);
    }

    fn placement(&self) -> ModalPlacement<'_> {
        let theme = self.theme;
        let measures = HistoryMeasures::of(self.entries);
        ModalPlacement {
            inset: Inset::default(),
            container: self.container,
            border_title: modal_title(
                HistoryGlyphs::default().title_word,
                format!("{} tracks", self.entries.len()),
                theme,
            ),
            modal_title: HistoryGlyphs::default().title_word,
            content_width: measures
                .natural_width(ModalMetrics::default().column_spacing),
            content_rows: u16::try_from(self.entries.len()).unwrap_or(u16::MAX),
            hint: None,
            theme,
            chrome: ModalChrome::default(),
        }
    }

    fn render_rows(&self, areas: ModalScrollAreas, buffer: &mut Buffer) {
        let colors = ModalRowColors::from_theme(&self.theme);
        let table_area = areas.rows;
        let lead = lead_cells(&areas);
        let total = self.entries.len();
        let height = usize::from(table_area.height);
        let columns = HistoryColumns::resolve(
            column_width(&areas),
            ModalMetrics::default().column_spacing,
        );
        let offset = scroll_offset(self.selected, total, height);

        let table = Table::new(
            self.entries.iter().map(|played| {
                entry_row(
                    &EntryRow {
                        played,
                        columns,
                        lead,
                    },
                    colors,
                    self.now_unix,
                )
            }),
            columns.constraints(),
        )
        .column_spacing(columns.spacing)
        .row_highlight_style(colors.highlight());
        let mut table_rows = TableState::new()
            .with_offset(offset)
            .with_selected(Some(self.selected));
        StatefulWidget::render(table, table_area, buffer, &mut table_rows);

        render_scrollbar(
            areas.scrollbar,
            ScrollbarTrack {
                total,
                offset,
                viewport: height,
                thumb: self.theme.frame(),
                track: colors.dim,
            },
            buffer,
        );
    }
}

impl Widget for &HistoryOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
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
                    HistoryGlyphs::default().label_separator,
                    played.title
                )
            },
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WhenColumn {
    cells: u16,
}

impl Default for WhenColumn {
    fn default() -> Self {
        Self { cells: 8 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistoryMeasures {
    label: u16,
}

impl HistoryMeasures {
    fn of(view: &[HistoryEntry]) -> Self {
        let widest = view
            .iter()
            .map(|played| played_label(played).width())
            .max()
            .unwrap_or(0);
        Self {
            label: u16::try_from(widest).unwrap_or(u16::MAX),
        }
    }

    fn natural_width(self, spacing: u16) -> u16 {
        if self.label == 0 {
            let placeholder = HistoryGlyphs::default().empty_placeholder.width();
            return u16::try_from(placeholder).unwrap_or(u16::MAX);
        }
        HistoryColumns {
            label: self.label,
            when: WhenColumn::default().cells,
            spacing,
        }
        .total()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistoryColumns {
    label: u16,
    when: u16,
    spacing: u16,
}

impl HistoryColumns {
    fn resolve(width: u16, spacing: u16) -> Self {
        let when = WhenColumn::default().cells;
        Self {
            label: width.saturating_sub(when.saturating_add(spacing)),
            when,
            spacing,
        }
    }

    fn total(self) -> u16 {
        self.label
            .saturating_add(self.when)
            .saturating_add(self.spacing)
    }

    fn constraints(self) -> [Constraint; 2] {
        [
            Constraint::Length(self.label),
            Constraint::Length(self.when),
        ]
    }
}

struct EntryRow<'a> {
    played: &'a HistoryEntry,
    columns: HistoryColumns,
    lead: u16,
}

fn when_label(played: &HistoryEntry, now_unix: u64) -> String {
    let then_unix = u64::try_from(played.at).unwrap_or(0);
    relative_time(now_unix, then_unix)
}

fn entry_row(
    row: &EntryRow<'_>,
    colors: ModalRowColors,
    now_unix: u64,
) -> Row<'static> {
    let [label, when] = entry_cells(row, now_unix);
    Row::new(vec![
        Line::from(label).style(Style::default().fg(colors.text)),
        Line::from(when)
            .right_aligned()
            .style(Style::default().fg(colors.dim)),
    ])
}

fn entry_cells(row: &EntryRow<'_>, now_unix: u64) -> [String; 2] {
    let columns = row.columns;
    let cell = |value: &str, width: u16| {
        truncate_to_width(value, usize::from(width), TruncateGlyphs::default())
            .into_owned()
    };
    [
        led(&played_label(row.played), row.lead, columns.label),
        cell(&when_label(row.played, now_unix), columns.when),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::domain::HistoryEntry;
    use ratatui::layout::Rect;

    use crate::{
        overlay::{history::HistoryOverlay, modal::OverlayContainer},
        scene::fixtures::{find_text, noir, painted, painted_buffer},
        theme::{ActiveTheme, ColorDepth},
    };

    fn entry(path: &str, title: &str, artist: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            path: path.into(),
            title: title.to_string(),
            artist: artist.map(str::to_string),
            at: 0,
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
        let overlay = HistoryOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            selected: 1,
            now_unix: 0,
            container: OverlayContainer::Modal { avoid: &[] },
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn history_overlay_highlights_the_selected_row_and_aligns_its_label_column() {
        let theme = noir();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let entries = [
            entry("/m/a.flac", "Alpha", Some("Artist A")),
            entry("/m/b.flac", "Beta", None),
        ];
        let overlay = HistoryOverlay {
            theme: active,
            entries: &entries,
            selected: 1,
            now_unix: 0,
            container: OverlayContainer::Modal { avoid: &[] },
        };
        let buffer = painted_buffer(&overlay, 80, 28);
        let selection_bg = active.selection_bg();
        let (alpha_x, alpha_y) = find_text(&buffer, "Artist A — Alpha").unwrap();
        let (beta_x, beta_y) = find_text(&buffer, "Beta").unwrap();
        assert_eq!(buffer[(beta_x, beta_y)].style().bg, Some(selection_bg));
        assert_ne!(buffer[(alpha_x, alpha_y)].style().bg, Some(selection_bg));
        assert_eq!(alpha_x, beta_x);
    }

    #[test]
    fn history_overlay_with_a_scrollbar_keeps_its_time_column_clear_of_it() {
        let theme = noir();
        let entries = scrolling_entries();
        let overlay = HistoryOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            selected: 0,
            now_unix: 0,
            container: OverlayContainer::Pane(Rect::new(0, 0, 120, 40)),
        };
        insta::assert_snapshot!(painted(&overlay, 120, 40));
    }

    #[test]
    fn the_history_time_column_never_touches_the_scrollbar() {
        let theme = noir();
        let entries = scrolling_entries();
        let overlay = HistoryOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            selected: 0,
            now_unix: 0,
            container: OverlayContainer::Pane(Rect::new(0, 0, 120, 40)),
        };
        let buffer = painted_buffer(&overlay, 120, 40);
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
        let overlay = HistoryOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            selected: 0,
            now_unix: 0,
            container: OverlayContainer::Modal { avoid: &[] },
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn history_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let entries: [HistoryEntry; 0] = [];
        let overlay = HistoryOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            entries: &entries,
            selected: 0,
            now_unix: 0,
            container: OverlayContainer::Modal { avoid: &[] },
        };
        let _ = painted(&overlay, 4, 3);
    }
}
