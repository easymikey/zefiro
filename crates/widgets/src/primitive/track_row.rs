use kernel::domain::geometry::Cells;
use ratatui::{style::Style, text::Line};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        glyphs,
        marker::{
            FAVORITE_COLUMNS,
            Favorite,
            MARKERS_WIDTH,
            PLAYING_COLUMNS,
            QueueNumber,
            column_padding,
            favorite_marker,
        },
        span::{line, text},
        truncate::{blanks, truncate},
    },
    theme::active_theme::ActiveTheme,
};

const CHIP_GAP: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selected {
    Yes,
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Playing {
    Yes,
    No,
}

#[must_use]
pub(crate) fn playing_marker(playing: Playing) -> &'static str {
    match playing {
        Playing::Yes => glyphs::playlist::PLAYING,
        Playing::No => "",
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TrackRow<'a> {
    pub(crate) title: &'a str,
    pub(crate) selected: Selected,
    pub(crate) favorite: Favorite,
    pub(crate) playing: Playing,
    pub(crate) queued_number: Option<QueueNumber>,
    pub(crate) row_width: Cells,
}

#[must_use]
pub(crate) fn track_row_line<'a>(
    track_row: &TrackRow<'a>,
    theme: &ActiveTheme<'_>,
) -> Line<'a> {
    let colors = theme.colors();
    let favorite_width = usize::from(FAVORITE_COLUMNS);
    let playing_width = usize::from(PLAYING_COLUMNS);
    let fav = favorite_marker(track_row.favorite);
    let playing = playing_marker(track_row.playing);
    let markers_width = usize::from(MARKERS_WIDTH);
    let body_width = track_row.row_width.count().saturating_sub(markers_width);
    let chip = track_row
        .queued_number
        .into_iter()
        .flat_map(QueueNumber::chip);
    let chip_width: usize = chip.clone().map(UnicodeWidthStr::width).sum();
    let title_width = match track_row.queued_number {
        Some(_) => body_width
            .saturating_sub(chip_width)
            .saturating_sub(CHIP_GAP),
        None => body_width,
    };
    let title = truncate(track_row.title, title_width);
    let gap = if track_row.queued_number.is_none() || title.is_empty() {
        0
    } else {
        CHIP_GAP
    };
    let row_style = match track_row.selected {
        Selected::Yes => Style::default().fg(colors.selection_foreground),
        Selected::No => Style::default().fg(colors.foreground),
    };
    let fixed = [
        text(fav).fg(theme.favorite()),
        text(blanks(column_padding(fav, favorite_width))).style(row_style),
        text(playing).style(row_style),
        text(blanks(column_padding(playing, playing_width))).style(row_style),
        text(title).style(row_style),
        text(blanks(gap)).style(row_style),
    ];
    line(
        fixed
            .into_iter()
            .chain(chip.map(|piece| text(piece).fg(colors.highlight))),
    )
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use kernel::domain::geometry::Cells;
    use unicode_width::UnicodeWidthStr;

    use crate::{
        primitive::{
            marker::{Favorite, MARKERS_WIDTH, QueueNumber},
            track_row::{Playing, Selected, TrackRow, track_row_line},
        },
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn base_props(title: &str, row_width: Cells) -> TrackRow<'_> {
        TrackRow {
            title,
            selected: Selected::No,
            favorite: Favorite::No,
            playing: Playing::No,
            queued_number: None,
            row_width,
        }
    }

    #[test]
    fn long_ascii_title_truncates_to_row_width_with_ellipsis() {
        let row_width = Cells(20);
        let text = track_row_line(
            &base_props("a very long track title that will not fit", row_width),
            &ActiveTheme::new(&noir(), ColorDepth::TrueColor),
        )
        .to_string();
        assert_eq!(text.width(), row_width.count());
        assert!(text.ends_with('…'));
        let fixed_width = usize::from(MARKERS_WIDTH);
        assert_eq!(
            text.get(..fixed_width),
            Some(" ".repeat(fixed_width).as_str())
        );
    }

    #[test]
    fn a_title_that_fits_is_borrowed_not_copied() {
        let line = track_row_line(
            &base_props("song", Cells(20)),
            &ActiveTheme::new(&noir(), ColorDepth::TrueColor),
        );
        assert!(
            line.spans
                .iter()
                .any(|span| matches!(&span.content, Cow::Borrowed("song")))
        );
    }

    #[test]
    fn the_chip_follows_the_title_with_one_space_and_carries_the_position() {
        let mut view = base_props("song", Cells(20));
        view.queued_number = Some(QueueNumber::new(12));
        let text =
            track_row_line(&view, &ActiveTheme::new(&noir(), ColorDepth::TrueColor))
                .to_string();
        assert!(text.ends_with("song [q12]"));
    }

    #[test]
    fn a_title_too_long_for_the_row_truncates_so_the_chip_still_follows_it() {
        let row_width = Cells(20);
        let mut view = base_props("a very long track title", row_width);
        view.queued_number = Some(QueueNumber::new(1));
        let text =
            track_row_line(&view, &ActiveTheme::new(&noir(), ColorDepth::TrueColor))
                .to_string();
        assert_eq!(text.width(), row_width.count());
        assert!(text.ends_with("… [q1]"));
    }

    #[test]
    fn cjk_title_truncates_on_a_cell_boundary() {
        let row_width = Cells(12);
        let fixed_width = usize::from(MARKERS_WIDTH);
        let title_width = row_width.count() - fixed_width;
        let text = track_row_line(
            &base_props("界界界界界界界界", row_width),
            &ActiveTheme::new(&noir(), ColorDepth::TrueColor),
        )
        .to_string();
        let title_part = text.get(fixed_width..).unwrap_or("").trim_end();
        assert!(title_part.width() <= title_width);
        assert!(title_part.ends_with('…'));
    }
}
