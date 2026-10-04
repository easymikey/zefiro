use ratatui::{
    style::{Color, Style},
    text::Line,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    Playing,
    primitive::{
        chip,
        marker::{
            FAVORITE_COLUMNS,
            Favorite,
            MARKERS_WIDTH,
            PLAYING_COLUMNS,
            QueuePosition,
            column_padding,
            favorite_marker,
            playing_marker,
        },
        span::{line, text},
        text::{blanks, truncate},
    },
    theme::{ActiveTheme, Role},
};

const CHIP_GAP: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selected {
    Yes,
    No,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TrackRowView<'a> {
    pub(crate) title: &'a str,
    pub(crate) selected: Selected,
    pub(crate) favorite: Favorite,
    pub(crate) playing: Playing,
    pub(crate) queued: Option<QueuePosition>,
    pub(crate) row_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrackRowStyle {
    pub(crate) foreground: Color,
    pub(crate) selected_foreground: Color,
    pub(crate) selected_background: Color,
    pub(crate) favorite: Color,
    pub(crate) highlight: Color,
}

impl TrackRowStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            foreground: theme.role(Role::Text),
            selected_foreground: theme.role(Role::SelectionForeground),
            selected_background: theme.role(Role::SelectionBackground),
            favorite: theme.favorite(),
            highlight: theme.role(Role::Highlight),
        }
    }
}

#[must_use]
pub(crate) fn track_row_line(
    view: &TrackRowView<'_>,
    style: TrackRowStyle,
) -> Line<'static> {
    let favorite_width = usize::from(FAVORITE_COLUMNS);
    let playing_width = usize::from(PLAYING_COLUMNS);
    let fav = favorite_marker(view.favorite);
    let playing = playing_marker(view.playing);
    let markers_width = usize::from(MARKERS_WIDTH);
    let body_width = view.row_width.saturating_sub(markers_width);
    let chip = view
        .queued
        .map(|position| chip::compact(&position.label()))
        .unwrap_or_default();
    let title_width = if chip.is_empty() {
        body_width
    } else {
        body_width
            .saturating_sub(chip.width())
            .saturating_sub(CHIP_GAP)
    };
    let title = truncate(view.title, title_width).into_owned();
    let gap = if chip.is_empty() || title.is_empty() {
        0
    } else {
        CHIP_GAP
    };
    let row_style = match view.selected {
        Selected::Yes => Style::default().fg(style.selected_foreground),
        Selected::No => Style::default().fg(style.foreground),
    };
    line([
        text(fav).fg(style.favorite),
        text(blanks(column_padding(fav, favorite_width))).style(row_style),
        text(playing).style(row_style),
        text(blanks(column_padding(playing, playing_width))).style(row_style),
        text(title).style(row_style),
        text(blanks(gap)).style(row_style),
        text(chip).fg(style.highlight),
    ])
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use crate::{
        Playing,
        primitive::{
            marker::{Favorite, MARKERS_WIDTH, QueuePosition},
            track_row::{Selected, TrackRowStyle, TrackRowView, track_row_line},
        },
        test_support::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn colors() -> TrackRowStyle {
        TrackRowStyle::from_theme(&ActiveTheme::new(&noir(), ColorDepth::TrueColor))
    }

    fn base_props(title: &str, row_width: usize) -> TrackRowView<'_> {
        TrackRowView {
            title,
            selected: Selected::No,
            favorite: Favorite::No,
            playing: Playing::No,
            queued: None,
            row_width,
        }
    }

    #[test]
    fn long_ascii_title_truncates_to_row_width_with_ellipsis() {
        let row_width = 20;
        let text = track_row_line(
            &base_props("a very long track title that will not fit", row_width),
            colors(),
        )
        .to_string();
        assert_eq!(text.width(), row_width);
        assert!(text.ends_with('…'));
        let fixed_width = usize::from(MARKERS_WIDTH);
        assert_eq!(
            text.get(..fixed_width),
            Some(" ".repeat(fixed_width).as_str())
        );
    }

    #[test]
    fn the_chip_follows_the_title_with_one_space_and_carries_the_position() {
        let mut view = base_props("song", 20);
        view.queued = Some(QueuePosition::new(12));
        let text = track_row_line(&view, colors()).to_string();
        assert!(text.ends_with("song [q12]"));
    }

    #[test]
    fn a_title_too_long_for_the_row_truncates_so_the_chip_still_follows_it() {
        let row_width = 20;
        let mut view = base_props("a very long track title", row_width);
        view.queued = Some(QueuePosition::new(1));
        let text = track_row_line(&view, colors()).to_string();
        assert_eq!(text.width(), row_width);
        assert!(text.ends_with("… [q1]"));
    }

    #[test]
    fn cjk_title_truncates_on_a_cell_boundary() {
        let row_width = 12;
        let fixed_width = usize::from(MARKERS_WIDTH);
        let title_width = row_width - fixed_width;
        let text = track_row_line(&base_props("界界界界界界界界", row_width), colors())
            .to_string();
        let title_part = text.get(fixed_width..).unwrap_or_default().trim_end();
        assert!(title_part.width() <= title_width);
        assert!(title_part.ends_with('…'));
    }
}
