use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders},
};

use crate::{
    geometry::{Aspects, CellAspect, Cells, CoverSizing},
    primitive::inset::Inset,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CardLayout {
    pub(crate) column_gap: u16,
    pub(crate) height: u16,
    pub(crate) progress_height: u16,
    pub(crate) gap: u16,
    pub(crate) line_gap: u16,
    pub(crate) volume_bar_width: u16,
    pub(crate) spectrum_height: u16,
    pub(crate) volume_height: u16,
    pub(crate) spectrum_volume_gap: u16,
    pub(crate) spectrum_max_dots: u32,
    pub(crate) bracket_margin: u16,
    pub(crate) status_width: u16,
}

impl Default for CardLayout {
    fn default() -> Self {
        Self {
            column_gap: 2,
            height: 12,
            progress_height: 1,
            gap: 1,
            line_gap: 0,
            volume_bar_width: 24,
            spectrum_height: 3,
            volume_height: 1,
            spectrum_volume_gap: 2,
            spectrum_max_dots: 12,
            status_width: 11,
            bracket_margin: 1,
        }
    }
}

#[must_use]
pub(crate) fn height() -> u16 {
    CardLayout::default().height
}

pub(crate) fn inner(area: Rect) -> Rect {
    Block::default()
        .borders(Borders::ALL)
        .padding(Inset::card().padding())
        .inner(area)
}

fn cover_width_for_height(height: u16, aspects: Aspects) -> u16 {
    Cells::from_f32_floor((f32::from(height) * aspects.cell.0 * aspects.cover.0).ceil())
        .get()
}

#[must_use]
pub(crate) fn cover_cell_height(area: Rect, sizing: CoverSizing) -> u16 {
    let available = inner(area).height;
    match sizing {
        CoverSizing::Fixed { height, .. } => height.min(available),
        CoverSizing::Auto { .. } => available,
        CoverSizing::Off => 0,
    }
}

#[must_use]
pub(crate) fn cover_cell_width(
    area: Rect,
    cell_aspect: CellAspect,
    sizing: CoverSizing,
) -> u16 {
    match sizing {
        CoverSizing::Fixed { width, .. } => width,
        CoverSizing::Off => 0,
        CoverSizing::Auto { cover_aspect } => cover_width_for_height(
            cover_cell_height(area, sizing),
            Aspects {
                cell: cell_aspect,
                cover: cover_aspect,
            },
        ),
    }
}

fn cover_column_span(area: Rect, cell_aspect: CellAspect, sizing: CoverSizing) -> u16 {
    match sizing {
        CoverSizing::Off => 0,
        CoverSizing::Fixed { .. } | CoverSizing::Auto { .. } => {
            cover_cell_width(area, cell_aspect, sizing)
                + CardLayout::default().column_gap
        }
    }
}

#[cfg(test)]
#[must_use]
pub(crate) fn text_width(
    area: Rect,
    cell_aspect: CellAspect,
    sizing: CoverSizing,
) -> u16 {
    inner(area)
        .width
        .saturating_sub(cover_column_span(area, cell_aspect, sizing))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardMetrics {
    pub cover_square: Rect,
    pub content_column: Rect,
    pub row_width: u16,
    pub status_row: Rect,
    pub title_row: Rect,
    pub artist_row: Rect,
    pub time_row: Rect,
    pub progress_row: Rect,
    pub spectrum_row: Rect,
    pub volume_row: Rect,
}

fn row_bands(column: Rect, layout: &CardLayout) -> [Rect; 8] {
    column.layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(layout.line_gap),
        Constraint::Length(1),
        Constraint::Length(layout.line_gap),
        Constraint::Length(1),
        Constraint::Length(layout.gap),
        Constraint::Length(layout.progress_height),
        Constraint::Length(layout.spectrum_height),
    ]))
}

fn meter_and_volume(band: Rect, layout: &CardLayout) -> (Rect, Rect) {
    let [spectrum_row, volume_column] = band.layout(
        &Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(layout.volume_bar_width.min(band.width)),
        ])
        .spacing(layout.spectrum_volume_gap),
    );
    let [_, volume_row] = volume_column.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(layout.volume_height),
    ]));
    (spectrum_row, volume_row)
}

#[must_use]
pub(crate) fn metrics(
    area: Rect,
    cell_aspect: CellAspect,
    sizing: CoverSizing,
) -> CardMetrics {
    let layout = CardLayout::default();
    let [cover_column, content_column] = inner(area).layout(&Layout::horizontal([
        Constraint::Length(cover_column_span(area, cell_aspect, sizing)),
        Constraint::Min(0),
    ]));
    let cover_square = Rect {
        x: cover_column.x,
        y: cover_column.y,
        width: cover_cell_width(area, cell_aspect, sizing),
        height: cover_cell_height(area, sizing),
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
    ] = row_bands(content_column, &layout);
    let [title_row, status_row] = title_band.layout(&Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(layout.status_width.min(row_width)),
    ]));
    let (spectrum_row, volume_row) = meter_and_volume(spectrum_band, &layout);

    CardMetrics {
        cover_square,
        content_column,
        row_width,
        status_row,
        title_row,
        artist_row,
        time_row,
        progress_row,
        spectrum_row,
        volume_row,
    }
}

pub(crate) fn content_rect(metrics: &CardMetrics) -> Rect {
    let top = metrics.title_row.y;
    let bottom = metrics.spectrum_row.y + metrics.spectrum_row.height;
    Rect {
        x: metrics.content_column.x,
        y: top,
        width: metrics.row_width,
        height: bottom.saturating_sub(top),
    }
}

#[cfg(test)]
#[must_use]
pub(crate) fn info_rect(
    area: Rect,
    cell_aspect: CellAspect,
    sizing: CoverSizing,
) -> Rect {
    content_rect(&metrics(area, cell_aspect, sizing))
}

#[cfg(test)]
mod tests {
    use raster::{VinylLayout, canvas_aspect_ratio};
    use ratatui::layout::Rect;

    use crate::{
        card::{
            meters::volume_cell_size,
            metrics::{
                CardLayout,
                cover_cell_height,
                cover_cell_width,
                cover_column_span,
                info_rect,
                inner,
                metrics,
                text_width,
            },
        },
        geometry::{CellAspect, CoverAspect, CoverSizing},
    };

    #[test]
    fn cover_cell_width_rounds_up_so_the_vinyl_canvas_never_clips() {
        let layout = VinylLayout::default();
        let cover_aspect = CoverAspect(canvas_aspect_ratio(&layout));
        let cell_aspect = CellAspect(2.0);
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };
        let sizing = CoverSizing::Auto { cover_aspect };
        assert_eq!(cover_cell_height(area, sizing), 8);
        let width = cover_cell_width(area, cell_aspect, sizing);
        assert!(
            width >= 20,
            "8 rows at 1.21:1 on a 2:1 cell needs >= 20 cell-widths to \
             avoid clipping the peeking disc, got {width}"
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
        let cell_aspect = CellAspect(2.0);
        let vinyl = CoverSizing::Auto {
            cover_aspect: CoverAspect(canvas_aspect_ratio(&VinylLayout::default())),
        };
        let plain = CoverSizing::Auto {
            cover_aspect: CoverAspect(1.0),
        };
        let layout = CardLayout::default();

        assert!(
            metrics(area, cell_aspect, plain).content_column.x
                < metrics(area, cell_aspect, vinyl).content_column.x,
            "a square cover column must start the text column further left \
             than the vinyl's wider one"
        );
        assert!(
            text_width(area, cell_aspect, plain) > text_width(area, cell_aspect, vinyl)
        );
        for sizing in [plain, vinyl] {
            assert_eq!(
                cover_column_span(area, cell_aspect, sizing),
                cover_cell_width(area, cell_aspect, sizing) + layout.column_gap,
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
        let cell_aspect = CellAspect(2.0);

        assert_eq!(cover_column_span(area, cell_aspect, CoverSizing::Off), 0);
        assert_eq!(
            text_width(area, cell_aspect, CoverSizing::Off),
            inner(area).width
        );
        assert!(
            metrics(area, cell_aspect, CoverSizing::Off)
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
        insta::assert_debug_snapshot!(metrics(
            area,
            CellAspect::default(),
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
        let cell_aspect = CellAspect::default();
        let sizing = CoverSizing::default();
        let card_metrics = metrics(area, cell_aspect, sizing);
        let layout = CardLayout::default();
        let column = card_metrics.content_column;

        let (_, cell_height) = volume_cell_size();
        assert_eq!(cell_height, layout.volume_height);
        assert_eq!(
            cell_height, layout.progress_height,
            "both meter rows are a single row"
        );

        assert_eq!(card_metrics.volume_row.height, layout.volume_height);
        assert_eq!(card_metrics.volume_row.width, layout.volume_bar_width);
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
                + layout.spectrum_volume_gap,
            card_metrics.volume_row.x,
            "one spectrum_volume_gap separates the meter from the bar"
        );

        let rect = info_rect(area, cell_aspect, sizing);
        assert_eq!(rect.x, column.x);
        assert_eq!(rect.y, card_metrics.title_row.y, "starts at the title row");
        assert_eq!(rect.width, card_metrics.row_width);
        assert_eq!(
            rect.y + rect.height,
            card_metrics.spectrum_row.y + card_metrics.spectrum_row.height,
            "ends at the equalizer/volume block's last row"
        );
    }
}
