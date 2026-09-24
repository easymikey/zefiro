use std::time::Duration;

use kernel::{
    domain::{ScanStatus, format_time},
    playlist::RepeatMode,
};
use ratatui::{style::Color, text::Line};

use crate::primitive::{
    glyphs::TruncateGlyphs,
    span::{Piece, row, text},
    text::truncate_line_to_width,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScanProgress<'a> {
    Done,
    Scanning(&'a str),
    Tagging { done: usize, total: usize },
}

impl<'a> ScanProgress<'a> {
    #[must_use]
    pub(crate) fn of(status: ScanStatus, scanning_label: &'a str) -> Self {
        match status {
            ScanStatus::Idle => Self::Done,
            ScanStatus::Scanning => Self::Scanning(scanning_label),
            ScanStatus::Tagging { done, total } => Self::Tagging { done, total },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shuffle {
    On,
    Off,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusLineView<'a> {
    pub(crate) shuffle: Shuffle,
    pub(crate) repeat_mode: RepeatMode,
    pub(crate) queue_len: usize,
    pub(crate) position: usize,
    pub(crate) total: usize,
    pub(crate) scan: ScanProgress<'a>,
    pub(crate) theme_name: &'a str,
    pub(crate) sleep_left: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusLineColors {
    pub(crate) frame: Color,
    pub(crate) dim: Color,
    pub(crate) accent: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StatusGlyphs {
    name: &'static str,
    separator: &'static str,
    flag_separator: &'static str,
}

impl Default for StatusGlyphs {
    fn default() -> Self {
        Self {
            name: "Playlist",
            separator: crate::primitive::glyphs::TITLE_SEPARATOR,
            flag_separator: " · ",
        }
    }
}

fn counts(status: StatusLineView<'_>) -> String {
    match status.scan {
        ScanProgress::Done => format!(
            "{}/{}",
            (status.position + 1).min(status.total),
            status.total
        ),
        ScanProgress::Scanning(label) => label.to_string(),
        ScanProgress::Tagging { done, total } => {
            format!("{total} tracks · tagging {done}/{total}")
        }
    }
}

#[must_use]
pub(crate) fn build(
    status: StatusLineView<'_>,
    colors: StatusLineColors,
    row_width: usize,
) -> Line<'static> {
    let glyphs = StatusGlyphs::default();
    let pos_total = counts(status);

    let shuffle = match status.shuffle {
        Shuffle::On => "on",
        Shuffle::Off => "off",
    };
    let repeat = <&'static str>::from(status.repeat_mode);

    let flag = |label: &'static str, value: String| -> Vec<Piece<'static>> {
        vec![
            text(format!("{label} ")).fg(colors.dim),
            text(value).fg(colors.accent),
        ]
    };
    let flag_separator = || text(glyphs.flag_separator).fg(colors.dim);

    let head = [
        text(glyphs.name).fg(colors.frame),
        text(glyphs.separator).fg(colors.dim),
        text(pos_total).fg(colors.accent),
        text(glyphs.separator).fg(colors.dim),
    ];
    let mut flags = vec![
        ("shuffle", shuffle.to_string()),
        ("repeat", repeat.to_string()),
        ("queue", status.queue_len.to_string()),
        ("theme", status.theme_name.to_string()),
    ];
    if let Some(sleep_left) = status.sleep_left {
        flags.push(("sleep", format_time(sleep_left)));
    }
    let pieces =
        head.into_iter()
            .chain(flags.into_iter().enumerate().flat_map(
                |(index, (label, value))| {
                    (index > 0)
                        .then(flag_separator)
                        .into_iter()
                        .chain(flag(label, value))
                },
            ));

    truncate_line_to_width(row(pieces), row_width, TruncateGlyphs::default())
}

#[cfg(test)]
mod tests {
    use kernel::playlist::RepeatMode;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    use crate::status_line::{
        ScanProgress,
        Shuffle,
        StatusLineColors,
        StatusLineView,
        build,
    };

    fn colors() -> StatusLineColors {
        StatusLineColors {
            frame: Color::Blue,
            dim: Color::Gray,
            accent: Color::Cyan,
        }
    }

    fn view() -> StatusLineView<'static> {
        StatusLineView {
            shuffle: Shuffle::On,
            repeat_mode: RepeatMode::All,
            queue_len: 7,
            position: 2,
            total: 12,
            scan: ScanProgress::Done,
            theme_name: "rose-pine",
            sleep_left: None,
        }
    }

    fn written(status: StatusLineView<'_>) -> String {
        build(status, colors(), 80)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn tagging_counts_the_tracks_whose_tags_are_already_read() {
        let status = StatusLineView {
            scan: ScanProgress::Tagging {
                done: 64,
                total: 128,
            },
            ..view()
        };
        insta::assert_snapshot!(written(status));
    }

    #[test]
    fn a_scan_in_flight_wears_the_theme_word() {
        let status = StatusLineView {
            scan: ScanProgress::Scanning("Scanning…"),
            ..view()
        };
        insta::assert_snapshot!(written(status));
    }

    #[test]
    fn build_snapshot() {
        let line = build(view(), colors(), 80);
        insta::assert_debug_snapshot!(line);
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let text: String = build(view(), colors(), 80)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            text,
            "Playlist ── 3/12 ── shuffle on · repeat all · queue 7 · theme rose-pine"
        );
    }

    #[test]
    fn an_armed_sleep_timer_adds_a_countdown_flag() {
        use std::time::Duration;

        let status = StatusLineView {
            sleep_left: Some(Duration::from_secs(14 * 60 + 59)),
            ..view()
        };
        let text: String = build(status, colors(), 100)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.ends_with("sleep 14:59"), "got {text:?}");
    }

    #[test]
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let budget = 24;
        let text: String = build(view(), colors(), budget)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.width() <= budget, "got {text:?}");
        assert!(text.ends_with('…'), "got {text:?}");
        assert!(text.starts_with("Playlist"), "got {text:?}");
    }
}
