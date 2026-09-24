use ratatui::{
    style::{Color, Style},
    text::Line,
};
use unicode_width::UnicodeWidthStr;

use crate::primitive::{
    chip,
    glyphs::{PlaylistGlyphs, TruncateGlyphs},
    marker::{
        Favorite,
        MarkerColumns,
        Playing,
        QueuePosition,
        column_padding,
        favorite_marker,
        playing_marker,
    },
    span::{row, text},
    text::{blanks, truncate_to_width},
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
    pub(crate) columns: MarkerColumns,
    pub(crate) glyphs: PlaylistGlyphs,
    pub(crate) row_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RowColors {
    pub(crate) text: Color,
    pub(crate) selection_text: Color,
    pub(crate) favorite: Color,
    pub(crate) queue: Color,
}

#[must_use]
pub(crate) fn build(view: &TrackRowView<'_>, colors: RowColors) -> Line<'static> {
    let favorite_width = usize::from(view.columns.favorite);
    let playing_width = usize::from(view.columns.playing);
    let fav = favorite_marker(view.favorite, view.glyphs);
    let playing = playing_marker(view.playing, view.glyphs);
    let markers_width = usize::from(view.columns.total());
    let body_width = view.row_width.saturating_sub(markers_width);
    let chip = view
        .queued
        .map(|position| chip::compact(&position.label(view.glyphs)))
        .unwrap_or_default();
    let title_width = if chip.is_empty() {
        body_width
    } else {
        body_width
            .saturating_sub(chip.width())
            .saturating_sub(CHIP_GAP)
    };
    let title = truncate_to_width(view.title, title_width, TruncateGlyphs::default())
        .into_owned();
    let gap = if chip.is_empty() || title.is_empty() {
        0
    } else {
        CHIP_GAP
    };
    let style = match view.selected {
        Selected::Yes => Style::default().fg(colors.selection_text),
        Selected::No => Style::default().fg(colors.text),
    };
    row([
        text(fav).fg(colors.favorite),
        text(blanks(column_padding(fav, favorite_width))).style(style),
        text(playing).style(style),
        text(blanks(column_padding(playing, playing_width))).style(style),
        text(title).style(style),
        text(blanks(gap)).style(style),
        text(chip).fg(colors.queue),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    use crate::primitive::{
        glyphs::PlaylistGlyphs,
        marker::{Favorite, MarkerColumns, Playing, QueuePosition},
        track_row::{RowColors, Selected, TrackRowView, build},
    };

    fn colors() -> RowColors {
        RowColors {
            text: Color::White,
            selection_text: Color::Yellow,
            favorite: Color::Magenta,
            queue: Color::Cyan,
        }
    }

    fn base_props(title: &str, row_width: usize) -> TrackRowView<'_> {
        TrackRowView {
            title,
            selected: Selected::No,
            favorite: Favorite::No,
            playing: Playing::No,
            queued: None,
            columns: MarkerColumns::default(),
            glyphs: PlaylistGlyphs::default(),
            row_width,
        }
    }

    fn rendered(view: &TrackRowView<'_>) -> String {
        build(view, colors())
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn long_ascii_title_truncates_to_row_width_with_ellipsis() {
        let row_width = 20;
        let text = rendered(&base_props(
            "a very long track title that will not fit",
            row_width,
        ));
        assert_eq!(text.width(), row_width);
        assert!(text.ends_with('…'));
        let fixed_width = usize::from(MarkerColumns::default().total());
        assert_eq!(
            text.get(..fixed_width),
            Some(" ".repeat(fixed_width).as_str())
        );
    }

    #[test]
    fn the_chip_follows_the_title_with_one_space_and_carries_the_position() {
        let mut view = base_props("song", 20);
        view.queued = Some(QueuePosition::new(12));
        let text = rendered(&view);
        assert!(text.ends_with("song [q12]"));
    }

    #[test]
    fn a_title_too_long_for_the_row_truncates_so_the_chip_still_follows_it() {
        let row_width = 20;
        let mut view = base_props("a very long track title", row_width);
        view.queued = Some(QueuePosition::new(1));
        let text = rendered(&view);
        assert_eq!(text.width(), row_width);
        assert!(text.ends_with("… [q1]"));
    }

    #[test]
    fn cjk_title_truncates_on_a_cell_boundary() {
        let row_width = 12;
        let fixed_width = usize::from(MarkerColumns::default().total());
        let title_width = row_width - fixed_width;
        let text = rendered(&base_props("界界界界界界界界", row_width));
        let title_part = text.get(fixed_width..).unwrap_or_default().trim_end();
        assert!(title_part.width() <= title_width);
        assert!(title_part.ends_with('…'));
    }
}
