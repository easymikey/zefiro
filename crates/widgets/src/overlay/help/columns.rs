use ratatui::{
    layout::{Constraint, Rect},
    style::Color,
    text::Line,
    widgets::{Cell, Row},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::{
        help::groups::{HelpGroup, HelpGroups, HelpLayout, small_count_u16},
        modal::{Hint, list_capacity},
    },
    primitive::{
        glyphs::HelpGlyphs,
        span::{line, text},
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HelpColors {
    pub(crate) title: Color,
    pub(crate) key: Color,
    pub(crate) description: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HelpColumn {
    pub(crate) rows: Vec<Row<'static>>,
    chord_width: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl HelpColumn {
    pub(crate) fn constraints(&self) -> [Constraint; 2] {
        [Constraint::Length(self.chord_width), Constraint::Min(0)]
    }
}

fn widest_chord(groups: &[&HelpGroup]) -> usize {
    groups
        .iter()
        .flat_map(|group| group.bindings.iter())
        .map(|(chord, _)| chord.width())
        .max()
        .unwrap_or(0)
}

fn full_width_row(line: Line<'static>) -> Row<'static> {
    Row::new(vec![Cell::from(line).column_span(2)])
}

fn column_lines(
    groups: &[&HelpGroup],
    colors: HelpColors,
    layout: HelpLayout,
) -> HelpColumn {
    let chord_width = widest_chord(groups);
    let mut rows: Vec<Row<'static>> = Vec::new();
    let mut max_width = 0usize;
    for (group_index, group) in groups.iter().enumerate() {
        if group_index > 0 {
            rows.push(Row::default());
        }
        max_width = max_width.max(group.title.width());
        rows.push(full_width_row(line([text(group.title)
            .fg(colors.title)
            .bold()])));
        for (key, description) in &group.bindings {
            max_width = max_width
                .max(chord_width + usize::from(layout.chord_gap) + description.width());
            rows.push(Row::new(vec![
                Cell::from(line([text(key.clone()).fg(colors.key)]).right_aligned()),
                Cell::from(line([text(description.clone()).fg(colors.description)])),
            ]));
        }
    }
    let height = small_count_u16(rows.len());
    HelpColumn {
        rows,
        chord_width: small_count_u16(chord_width),
        width: small_count_u16(max_width),
        height,
    }
}

fn height_spread(first: u16, second: u16, third: u16) -> u16 {
    first.max(second).max(third) - first.min(second).min(third)
}

fn three_columns(
    groups: [&HelpGroup; 4],
    colors: HelpColors,
    layout: HelpLayout,
) -> (HelpColumn, HelpColumn, HelpColumn) {
    let [playback, general, navigation, playlist] = groups;
    let playback_column = column_lines(&[playback], colors, layout);
    let general_and_navigation = column_lines(&[general, navigation], colors, layout);
    let playlist_alone = column_lines(&[playlist], colors, layout);
    let general_alone = column_lines(&[general], colors, layout);
    let navigation_and_playlist = column_lines(&[navigation, playlist], colors, layout);

    let navigation_after_general = height_spread(
        playback_column.height,
        general_and_navigation.height,
        playlist_alone.height,
    );
    let navigation_after_navigation = height_spread(
        playback_column.height,
        general_alone.height,
        navigation_and_playlist.height,
    );

    if navigation_after_general <= navigation_after_navigation {
        (playback_column, general_and_navigation, playlist_alone)
    } else {
        (playback_column, general_alone, navigation_and_playlist)
    }
}

#[derive(Debug, Clone, Copy)]
struct HelpHint {
    glyph: &'static str,
    color: Color,
}

fn fit_column(column: HelpColumn, available_height: u16, hint: HelpHint) -> HelpColumn {
    if available_height == 0 || column.height <= available_height {
        return column;
    }
    let keep = usize::from(available_height.saturating_sub(1));
    let mut rows = column.rows;
    rows.truncate(keep);
    rows.push(full_width_row(line([text(hint.glyph).fg(hint.color)])));
    HelpColumn {
        rows,
        chord_width: column.chord_width,
        width: column.width.max(small_count_u16(hint.glyph.width())),
        height: available_height,
    }
}

fn available_width(full: Rect) -> u16 {
    list_capacity(full, Hint::Absent).0
}

fn columns_width(columns: &[HelpColumn], column_gap: u16) -> u16 {
    let content = columns
        .iter()
        .fold(0u16, |total, column| total.saturating_add(column.width));
    content.saturating_add(
        column_gap.saturating_mul(small_count_u16(columns.len().saturating_sub(1))),
    )
}

fn squeezed_width(columns: &[HelpColumn], layout: HelpLayout) -> u16 {
    let content = columns.iter().fold(0u16, |total, column| {
        total.saturating_add(
            column
                .chord_width
                .saturating_add(layout.chord_gap)
                .saturating_add(layout.minimum_description),
        )
    });
    content.saturating_add(
        layout
            .column_gap
            .saturating_mul(small_count_u16(columns.len().saturating_sub(1))),
    )
}

fn columns_that_fit(
    candidates: Vec<Vec<HelpColumn>>,
    inner: u16,
    layout: HelpLayout,
) -> Vec<HelpColumn> {
    let last = candidates.len().saturating_sub(1);
    let natural = candidates
        .iter()
        .position(|candidate| columns_width(candidate, layout.column_gap) <= inner);
    let squeezed = candidates
        .iter()
        .position(|candidate| squeezed_width(candidate, layout) <= inner);
    let picked = match natural {
        Some(index) if index < last => Some(index),
        Some(_) | None => squeezed.or(natural),
    };
    let mut fallback = Vec::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        if picked == Some(index) {
            return candidate;
        }
        fallback = candidate;
    }
    fallback
}

pub(crate) struct HelpColumnFit<'a> {
    pub(crate) groups: &'a HelpGroups,
    pub(crate) colors: HelpColors,
    pub(crate) layout: HelpLayout,
    pub(crate) glyphs: HelpGlyphs,
    pub(crate) full: Rect,
}

pub(crate) fn select_help_columns(input: &HelpColumnFit<'_>) -> Vec<HelpColumn> {
    let HelpColumnFit {
        groups,
        colors,
        layout,
        glyphs,
        full,
    } = *input;
    let HelpGroups {
        playback,
        navigation,
        playlist,
        general,
    } = groups;

    let (_, available_height) = list_capacity(full, Hint::Absent);

    let single =
        column_lines(&[playback, navigation, playlist, general], colors, layout);
    let columns: Vec<HelpColumn> = if single.height <= available_height {
        vec![single]
    } else {
        let (left, middle, right) =
            three_columns([playback, general, navigation, playlist], colors, layout);
        columns_that_fit(
            vec![
                vec![left, middle, right],
                vec![
                    column_lines(&[playback], colors, layout),
                    column_lines(&[general, navigation, playlist], colors, layout),
                ],
                vec![single],
            ],
            available_width(full),
            layout,
        )
    };

    columns
        .into_iter()
        .map(|column| {
            fit_column(
                column,
                available_height,
                HelpHint {
                    glyph: glyphs.overflow_hint,
                    color: colors.key,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use ratatui::{layout::Rect, style::Color};
    use rstest::rstest;

    use crate::overlay::help::{
        columns::{
            HelpColors,
            HelpColumn,
            available_width,
            column_lines,
            columns_that_fit,
            columns_width,
            squeezed_width,
            three_columns,
        },
        groups::{HelpGroup, HelpLayout},
    };

    fn colors() -> HelpColors {
        HelpColors {
            title: Color::White,
            key: Color::White,
            description: Color::White,
        }
    }

    fn synthetic_group(title: &'static str, row_count: usize) -> HelpGroup {
        HelpGroup {
            title,
            bindings: (0..row_count)
                .map(|index| {
                    (format!("k{index}"), Cow::Owned(format!("Do thing {index}")))
                })
                .collect(),
        }
    }

    struct HelpRow {
        group_rows: [usize; 4],
        heights: (u16, u16, u16),
    }

    #[rstest]
    #[case::navigation_joins_the_middle(HelpRow { group_rows: [18, 8, 2, 16], heights: (19, 13, 17) })]
    #[case::navigation_joins_the_last(HelpRow { group_rows: [19, 14, 7, 2], heights: (20, 15, 12) })]
    fn three_columns_moves_navigation_to_the_column_that_balances_better(
        #[case] row: HelpRow,
    ) {
        let [playback_rows, general_rows, navigation_rows, playlist_rows] =
            row.group_rows;
        let playback = synthetic_group("Playback", playback_rows);
        let general = synthetic_group("General", general_rows);
        let navigation = synthetic_group("Navigation", navigation_rows);
        let playlist = synthetic_group("Playlist", playlist_rows);
        let (left, middle, right) = three_columns(
            [&playback, &general, &navigation, &playlist],
            colors(),
            HelpLayout::default(),
        );
        assert_eq!((left.height, middle.height, right.height), row.heights);
    }

    #[test]
    fn two_columns_of_comparable_groups_stay_within_a_few_rows_of_each_other() {
        let left_first = synthetic_group("A", 5);
        let left_second = synthetic_group("B", 4);
        let right_first = synthetic_group("C", 5);
        let right_second = synthetic_group("D", 4);
        let left = column_lines(
            &[&left_first, &left_second],
            colors(),
            HelpLayout::default(),
        );
        let right = column_lines(
            &[&right_first, &right_second],
            colors(),
            HelpLayout::default(),
        );
        assert!(
            left.height.abs_diff(right.height) <= 3,
            "columns should stay balanced: left={} right={}",
            left.height,
            right.height
        );
    }

    fn column_gap() -> u16 {
        HelpLayout::default().column_gap
    }

    fn layout() -> HelpLayout {
        HelpLayout::default()
    }

    fn column(width: u16) -> HelpColumn {
        HelpColumn {
            rows: Vec::new(),
            chord_width: 0,
            width,
            height: 0,
        }
    }

    fn candidates() -> Vec<Vec<HelpColumn>> {
        vec![
            vec![column(20), column(20), column(20)],
            vec![column(20), column(40)],
            vec![column(60)],
        ]
    }

    #[rstest]
    #[case::three_at_their_exact_width(Width::ThreeColumns, 0)]
    #[case::one_cell_short_of_three(Width::OneCellShortOfThree, 1)]
    #[case::two_squeezed(Width::TwoSqueezed, 1)]
    #[case::nothing_fits(Width::Nothing, 2)]
    fn columns_that_fit_takes_the_widest_arrangement_that_fits(
        #[case] width: Width,
        #[case] expected: usize,
    ) {
        let available = match width {
            Width::ThreeColumns => {
                columns_width(&candidates().swap_remove(0), column_gap())
            }
            Width::OneCellShortOfThree => {
                columns_width(&candidates().swap_remove(0), column_gap()) - 1
            }
            Width::TwoSqueezed => {
                squeezed_width(&candidates().swap_remove(1), layout())
            }
            Width::Nothing => 1,
        };
        assert_eq!(
            columns_that_fit(candidates(), available, layout()),
            candidates().swap_remove(expected)
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum Width {
        ThreeColumns,
        OneCellShortOfThree,
        TwoSqueezed,
        Nothing,
    }

    #[test]
    fn the_squeeze_is_narrower_than_the_natural_width() {
        let two = candidates().swap_remove(1);
        assert!(squeezed_width(&two, layout()) < columns_width(&two, column_gap()));
    }

    #[test]
    fn the_cover_rect_does_not_narrow_the_width_the_columns_are_measured_against() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        assert_eq!(available_width(full), 114);
    }
}
