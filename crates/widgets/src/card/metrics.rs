use kernel::domain::geometry::Cells;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders},
};

use crate::{geometry::CoverSizing, primitive::inset::Inset};

const COLUMN_GAP: u16 = 2;
const CARD_HEIGHT: u16 = 12;
const PROGRESS_HEIGHT: u16 = 1;
const GAP: u16 = 1;
const LINE_GAP: u16 = 0;
pub(crate) const VOLUME_BAR_WIDTH: u16 = 24;
const SPECTRUM_HEIGHT: u16 = 3;
pub(crate) const VOLUME_HEIGHT: u16 = 1;
const SPECTRUM_VOLUME_GAP: u16 = 2;
pub(crate) const SPECTRUM_MAX_DOTS: u32 = 12;
pub(crate) const BRACKET_MARGIN: u16 = 1;
const STATUS_WIDTH: u16 = 11;

#[must_use]
pub(crate) fn card_height() -> Cells {
    Cells(CARD_HEIGHT)
}

pub(crate) fn inner(area: Rect) -> Rect {
    Block::default()
        .borders(Borders::ALL)
        .padding(Inset::card().padding())
        .inner(area)
}

fn cover_width_for_height(height: Cells, cell_aspect: f32, cover_aspect: f32) -> Cells {
    Cells(crate::pixels::numeric::floor(
        (f32::from(height.0) * cell_aspect * cover_aspect).ceil(),
    ))
}

#[must_use]
pub(crate) fn cover_cell_height(area: Rect, sizing: CoverSizing) -> Cells {
    let available = Cells(inner(area).height);
    match sizing {
        CoverSizing::Fixed { height, .. } => height.min(available),
        CoverSizing::Auto(_) => available,
        CoverSizing::Off => Cells(0),
    }
}

#[must_use]
pub(crate) fn cover_cell_width(
    area: Rect,
    cell_aspect: f32,
    sizing: CoverSizing,
) -> Cells {
    match sizing {
        CoverSizing::Fixed { width, .. } => width,
        CoverSizing::Off => Cells(0),
        CoverSizing::Auto(cover_aspect) => cover_width_for_height(
            cover_cell_height(area, sizing),
            cell_aspect,
            cover_aspect,
        ),
    }
}

fn cover_column_span(area: Rect, cell_aspect: f32, sizing: CoverSizing) -> Cells {
    match sizing {
        CoverSizing::Off => Cells(0),
        CoverSizing::Fixed { .. } | CoverSizing::Auto(_) => {
            Cells(cover_cell_width(area, cell_aspect, sizing).0 + COLUMN_GAP)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardMetrics {
    pub cover_square: Rect,
    pub content_column: Rect,
    pub row_width: Cells,
    pub status_row: Rect,
    pub title_row: Rect,
    pub artist_row: Rect,
    pub time_row: Rect,
    pub progress_row: Rect,
    pub spectrum_row: Rect,
    pub volume_row: Rect,
}

fn row_bands(column: Rect) -> [Rect; 8] {
    column.layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(LINE_GAP),
        Constraint::Length(1),
        Constraint::Length(LINE_GAP),
        Constraint::Length(1),
        Constraint::Length(GAP),
        Constraint::Length(PROGRESS_HEIGHT),
        Constraint::Length(SPECTRUM_HEIGHT),
    ]))
}

fn meter_and_volume(band: Rect) -> (Rect, Rect) {
    let [spectrum_row, volume_column] = band.layout(
        &Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(VOLUME_BAR_WIDTH.min(band.width)),
        ])
        .spacing(SPECTRUM_VOLUME_GAP),
    );
    let [_, volume_row] = volume_column.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(VOLUME_HEIGHT),
    ]));
    (spectrum_row, volume_row)
}

impl CardMetrics {
    #[must_use]
    pub(crate) fn new(
        area: Rect,
        cell_aspect: f32,
        sizing: CoverSizing,
    ) -> CardMetrics {
        let [cover_column, content_column] = inner(area).layout(&Layout::horizontal([
            Constraint::Length(cover_column_span(area, cell_aspect, sizing).0),
            Constraint::Min(0),
        ]));
        let cover_square = Rect {
            x: cover_column.x,
            y: cover_column.y,
            width: cover_cell_width(area, cell_aspect, sizing).0,
            height: cover_cell_height(area, sizing).0,
        };

        let row_width = content_column.width;
        let [
            title_band,
            _,
            artist_row,
            _,
            time_row,
            _,
            progress_row,
            spectrum_band,
        ] = row_bands(content_column);
        let [title_row, status_row] = title_band.layout(&Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(STATUS_WIDTH.min(row_width)),
        ]));
        let (spectrum_row, volume_row) = meter_and_volume(spectrum_band);

        CardMetrics {
            cover_square,
            content_column,
            row_width: Cells(row_width),
            status_row,
            title_row,
            artist_row,
            time_row,
            progress_row,
            spectrum_row,
            volume_row,
        }
    }
}

pub(crate) fn content_rect(metrics: &CardMetrics) -> Rect {
    let top = metrics.title_row.y;
    let bottom = metrics.spectrum_row.y + metrics.spectrum_row.height;
    Rect {
        x: metrics.content_column.x,
        y: top,
        width: metrics.row_width.0,
        height: bottom.saturating_sub(top),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::geometry::Cells;
    use ratatui::layout::Rect;

    use crate::{
        card::metrics::{
            COLUMN_GAP,
            CardMetrics,
            PROGRESS_HEIGHT,
            SPECTRUM_VOLUME_GAP,
            VOLUME_BAR_WIDTH,
            VOLUME_HEIGHT,
            content_rect,
            cover_cell_height,
            cover_cell_width,
            cover_column_span,
            inner,
        },
        geometry::{CoverSizing, DEFAULT_CELL_ASPECT},
        pixels::vinyl::geometry::canvas_aspect_ratio,
    };

    #[test]
    fn cover_cell_width_rounds_up_so_the_vinyl_canvas_never_clips() {
        let cover_aspect = canvas_aspect_ratio();
        let cell_aspect = DEFAULT_CELL_ASPECT;
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        let sizing = CoverSizing::Auto(cover_aspect);
        assert_eq!(cover_cell_height(area, sizing), Cells(8));
        let width = cover_cell_width(area, cell_aspect, sizing);
        assert!(
            width >= Cells(20),
            "8 rows at 1.21:1 on a 2:1 cell needs >= 20 cell-widths to \
             avoid clipping the peeking disc, got {width:?}"
        );
    }

    #[test]
    fn the_cover_column_is_its_own_styles_width() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        let cell_aspect = DEFAULT_CELL_ASPECT;
        let vinyl_sizing = CoverSizing::Auto(canvas_aspect_ratio());
        let plain_sizing = CoverSizing::Auto(1.0);

        assert!(
            CardMetrics::new(area, cell_aspect, plain_sizing)
                .content_column
                .x
                < CardMetrics::new(area, cell_aspect, vinyl_sizing)
                    .content_column
                    .x,
            "a square cover column must start the text column further left \
             than the vinyl's wider one"
        );
        assert!(
            CardMetrics::new(area, cell_aspect, plain_sizing).row_width
                > CardMetrics::new(area, cell_aspect, vinyl_sizing).row_width
        );
        for sizing in [plain_sizing, vinyl_sizing] {
            assert_eq!(
                cover_column_span(area, cell_aspect, sizing),
                Cells(cover_cell_width(area, cell_aspect, sizing).0 + COLUMN_GAP),
                "the column is the cell plus exactly one gap, nothing more"
            );
        }
    }

    #[test]
    fn cover_off_reserves_no_column_and_no_gap() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        let cell_aspect = DEFAULT_CELL_ASPECT;

        assert_eq!(
            cover_column_span(area, cell_aspect, CoverSizing::Off),
            Cells(0)
        );
        assert_eq!(
            CardMetrics::new(area, cell_aspect, CoverSizing::Off).row_width,
            Cells(inner(area).width)
        );
        assert!(
            CardMetrics::new(area, cell_aspect, CoverSizing::Off)
                .cover_square
                .is_empty()
        );
    }

    #[test]
    fn the_card_metrics_are_one_table_of_rects() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        insta::assert_debug_snapshot!(CardMetrics::new(
            area,
            DEFAULT_CELL_ASPECT,
            CoverSizing::default()
        ));
    }

    #[test]
    fn the_rows_line_up_inside_the_info_column() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        let cell_aspect = DEFAULT_CELL_ASPECT;
        let sizing = CoverSizing::default();
        let card_metrics = CardMetrics::new(area, cell_aspect, sizing);
        let column = card_metrics.content_column;

        assert_eq!(
            VOLUME_HEIGHT, PROGRESS_HEIGHT,
            "both meter rows are a single row"
        );

        assert_eq!(card_metrics.volume_row.height, VOLUME_HEIGHT);
        assert_eq!(card_metrics.volume_row.width, VOLUME_BAR_WIDTH);
        assert_eq!(
            card_metrics.volume_row.x + card_metrics.volume_row.width,
            column.x + column.width,
            "the bar ends at the info column's right edge"
        );
        assert_eq!(
            card_metrics.volume_row.y + card_metrics.volume_row.height,
            card_metrics.spectrum_row.y + card_metrics.spectrum_row.height,
            "the bar's row is the equalizer block's last"
        );
        assert_eq!(
            card_metrics.spectrum_row.x
                + card_metrics.spectrum_row.width
                + SPECTRUM_VOLUME_GAP,
            card_metrics.volume_row.x,
            "one spectrum_volume_gap separates the meter from the bar"
        );

        let rect = content_rect(&card_metrics);
        assert_eq!(rect.x, column.x);
        assert_eq!(rect.y, card_metrics.title_row.y, "starts at the title row");
        assert_eq!(rect.width, card_metrics.row_width.0);
        assert_eq!(
            rect.y + rect.height,
            card_metrics.spectrum_row.y + card_metrics.spectrum_row.height,
            "ends at the equalizer/volume block's last row"
        );
    }
}
