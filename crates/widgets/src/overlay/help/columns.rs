use kernel::domain::geometry::Cells;
use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Cell, Row},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::{
        help::groups::{
            CHORD_GAP,
            COLUMN_GAP,
            HelpGroup,
            HelpGroups,
            HelpRow,
            MINIMUM_DESCRIPTION,
        },
        modal::place::{Hint, list_capacity},
    },
    pixels::numeric::small_count_u16,
    primitive::{
        glyphs,
        span::{StyledText, line, text},
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HelpColumn {
    pub(crate) rows: Vec<Row<'static>>,
    chord_width: Cells,
    pub(crate) width: Cells,
    pub(crate) height: Cells,
}

impl HelpColumn {
    pub(crate) fn constraints(&self) -> [Constraint; 2] {
        [Constraint::Length(self.chord_width.0), Constraint::Min(0)]
    }

    pub(crate) fn new(groups: &[&HelpGroup], theme: &ActiveTheme<'_>) -> HelpColumn {
        let colors = theme.colors();
        let chord_width = widest_chord(groups);
        let max_width = groups
            .iter()
            .flat_map(|group| {
                std::iter::once(group.title.width()).chain(group.help_rows.iter().map(
                    |row| chord_width + usize::from(CHORD_GAP) + row.label.width(),
                ))
            })
            .max()
            .unwrap_or(0);
        let rows: Vec<Row<'static>> = groups
            .iter()
            .enumerate()
            .flat_map(|(group_index, group)| {
                (group_index > 0)
                    .then(Row::default)
                    .into_iter()
                    .chain(std::iter::once(full_width_row(line([text(group.title)
                        .fg(colors.muted_foreground)
                        .bold()]))))
                    .chain(group.help_rows.iter().map(|HelpRow { chord, label }| {
                        Row::new(vec![
                            Cell::from(
                                line([text(chord.clone()).fg(theme.muted_accent())])
                                    .right_aligned(),
                            ),
                            Cell::from(line([
                                text(label.clone()).fg(colors.foreground)
                            ])),
                        ])
                    }))
            })
            .collect();
        let height = Cells(small_count_u16(rows.len()));
        Self {
            rows,
            chord_width: Cells(small_count_u16(chord_width)),
            width: Cells(small_count_u16(max_width)),
            height,
        }
    }
}

fn widest_chord(groups: &[&HelpGroup]) -> usize {
    groups
        .iter()
        .flat_map(|group| group.help_rows.iter())
        .map(|row| row.chord.width())
        .max()
        .unwrap_or(0)
}

fn full_width_row(line: Line<'static>) -> Row<'static> {
    Row::new(vec![Cell::from(line).column_span(2)])
}

fn height_spread(
    first_height: Cells,
    second_height: Cells,
    third_height: Cells,
) -> Cells {
    Cells(
        first_height.max(second_height).max(third_height).0
            - first_height.min(second_height).min(third_height).0,
    )
}

fn three_columns(
    groups: [&HelpGroup; 4],
    theme: &ActiveTheme<'_>,
) -> (HelpColumn, HelpColumn, HelpColumn) {
    let [playback, general, navigation, playlist] = groups;
    let playback_column = HelpColumn::new(&[playback], theme);
    let general_and_navigation_column = HelpColumn::new(&[general, navigation], theme);
    let playlist_column = HelpColumn::new(&[playlist], theme);
    let general_column = HelpColumn::new(&[general], theme);
    let navigation_and_playlist_column =
        HelpColumn::new(&[navigation, playlist], theme);

    let navigation_after_general = height_spread(
        playback_column.height,
        general_and_navigation_column.height,
        playlist_column.height,
    );
    let navigation_after_navigation = height_spread(
        playback_column.height,
        general_column.height,
        navigation_and_playlist_column.height,
    );

    if navigation_after_general <= navigation_after_navigation {
        (
            playback_column,
            general_and_navigation_column,
            playlist_column,
        )
    } else {
        (
            playback_column,
            general_column,
            navigation_and_playlist_column,
        )
    }
}

fn fit_column(
    column: HelpColumn,
    available_height: Cells,
    styled_text: StyledText<'static>,
) -> HelpColumn {
    if available_height == Cells(0) || column.height <= available_height {
        return column;
    }
    let keep = usize::from(available_height.0.saturating_sub(1));
    let hint_width = Cells(small_count_u16(Span::from(styled_text.clone()).width()));
    HelpColumn {
        rows: column
            .rows
            .into_iter()
            .take(keep)
            .chain(std::iter::once(full_width_row(line([styled_text]))))
            .collect(),
        chord_width: column.chord_width,
        width: column.width.max(hint_width),
        height: available_height,
    }
}

fn available_width(full: Rect) -> Cells {
    list_capacity(full, Hint::Hidden).width
}

fn columns_width(columns: &[HelpColumn], column_gap_width: Cells) -> Cells {
    let content = columns
        .iter()
        .fold(0u16, |total, column| total.saturating_add(column.width.0));
    Cells(
        content.saturating_add(
            column_gap_width
                .0
                .saturating_mul(small_count_u16(columns.len().saturating_sub(1))),
        ),
    )
}

fn squeezed_width(columns: &[HelpColumn]) -> Cells {
    let content = columns.iter().fold(0u16, |total, column| {
        total.saturating_add(
            column
                .chord_width
                .0
                .saturating_add(CHORD_GAP)
                .saturating_add(MINIMUM_DESCRIPTION),
        )
    });
    Cells(content.saturating_add(
        COLUMN_GAP.saturating_mul(small_count_u16(columns.len().saturating_sub(1))),
    ))
}

fn columns_that_fit(
    candidate_columns: Vec<Vec<HelpColumn>>,
    inner_width: Cells,
) -> Vec<HelpColumn> {
    let last = candidate_columns.len().saturating_sub(1);
    let natural = candidate_columns.iter().position(|candidate| {
        columns_width(candidate, Cells(COLUMN_GAP)) <= inner_width
    });
    let squeezed = candidate_columns
        .iter()
        .position(|candidate| squeezed_width(candidate) <= inner_width);
    let picked = match natural {
        Some(index) if index < last => Some(index),
        Some(_) | None => squeezed.or(natural),
    };
    candidate_columns
        .into_iter()
        .nth(picked.unwrap_or(last))
        .unwrap_or_else(Vec::new)
}

pub(crate) fn select_help_columns(
    groups: &HelpGroups,
    theme: &ActiveTheme<'_>,
    full: Rect,
) -> Vec<HelpColumn> {
    let HelpGroups {
        playback_group: playback,
        navigation_group: navigation,
        playlist_group: playlist,
        general_group: general,
    } = groups;

    let available_height = list_capacity(full, Hint::Hidden).height;

    let single_column =
        HelpColumn::new(&[playback, navigation, playlist, general], theme);
    let columns: Vec<HelpColumn> = if single_column.height <= available_height {
        vec![single_column]
    } else {
        let (left, middle, right) =
            three_columns([playback, general, navigation, playlist], theme);
        columns_that_fit(
            vec![
                vec![left, middle, right],
                vec![
                    HelpColumn::new(&[playback], theme),
                    HelpColumn::new(&[general, navigation, playlist], theme),
                ],
                vec![single_column],
            ],
            available_width(full),
        )
    };

    columns
        .into_iter()
        .map(|column| {
            fit_column(
                column,
                available_height,
                text(glyphs::help::OVERFLOW_HINT).fg(theme.muted_accent()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use kernel::domain::geometry::Cells;
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::help::{
            columns::{
                HelpColumn,
                available_width,
                columns_that_fit,
                columns_width,
                squeezed_width,
                three_columns,
            },
            groups::{COLUMN_GAP, HelpGroup, HelpRow},
        },
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn synthetic_group(title: &'static str, row_count: usize) -> HelpGroup {
        HelpGroup {
            title,
            help_rows: (0..row_count)
                .map(|index| HelpRow {
                    chord: format!("k{index}"),
                    label: Cow::Owned(format!("Do thing {index}")),
                })
                .collect(),
        }
    }

    struct Balance {
        group_rows: [usize; 4],
        heights: (Cells, Cells, Cells),
    }

    #[rstest]
    #[case::navigation_joins_the_middle(Balance { group_rows: [18, 8, 2, 16], heights: (Cells(19), Cells(13), Cells(17)) })]
    #[case::navigation_joins_the_last(Balance { group_rows: [19, 14, 7, 2], heights: (Cells(20), Cells(15), Cells(12)) })]
    fn three_columns_moves_navigation_to_the_column_that_balances_better(
        #[case] balance: Balance,
    ) {
        let [playback_rows, general_rows, navigation_rows, playlist_rows] =
            balance.group_rows;
        let playback = synthetic_group("Playback", playback_rows);
        let general = synthetic_group("General", general_rows);
        let navigation = synthetic_group("Navigation", navigation_rows);
        let playlist = synthetic_group("Playlist", playlist_rows);
        let (left, middle, right) = three_columns(
            [&playback, &general, &navigation, &playlist],
            &ActiveTheme::new(&noir(), ColorDepth::TrueColor),
        );
        assert_eq!((left.height, middle.height, right.height), balance.heights);
    }

    fn column(width: u16) -> HelpColumn {
        HelpColumn {
            rows: Vec::new(),
            chord_width: Cells(0),
            width: Cells(width),
            height: Cells(0),
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
                columns_width(&candidates().swap_remove(0), Cells(COLUMN_GAP))
            }
            Width::OneCellShortOfThree => Cells(
                columns_width(&candidates().swap_remove(0), Cells(COLUMN_GAP)).0 - 1,
            ),
            Width::TwoSqueezed => squeezed_width(&candidates().swap_remove(1)),
            Width::Nothing => Cells(1),
        };
        assert_eq!(
            columns_that_fit(candidates(), available),
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
        assert!(squeezed_width(&two) < columns_width(&two, Cells(COLUMN_GAP)));
    }

    #[test]
    fn the_cover_rect_does_not_narrow_the_width_the_columns_are_measured_against() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        assert_eq!(available_width(full), Cells(114));
    }
}
