use std::{borrow::Cow, time::Duration};

use kernel::{
    domain::{ScanStatus, Shuffle},
    playlist::RepeatMode,
};
use ratatui::{style::Color, text::Line};

use crate::{
    primitive::{
        glyphs::TruncateGlyphs,
        span::{StyledText, line, text},
        text::truncate_line_to_width,
    },
    redraw::ceil_minutes,
};

const SHUFFLE_LABEL: &str = "shuffle ";
const REPEAT_LABEL: &str = "repeat ";
const QUEUE_LABEL: &str = "queue ";
const THEME_LABEL: &str = "theme ";
const SLEEP_LABEL: &str = "sleep ";

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

fn sleep_label(left: Duration) -> String {
    format!("{}m", ceil_minutes(left))
}

#[must_use]
pub(crate) fn status_line<'a>(
    status: StatusLineView<'a>,
    colors: StatusLineColors,
    row_width: usize,
) -> Line<'a> {
    let glyphs = StatusGlyphs::default();
    let pos_total = counts(status);

    let shuffle: &'static str = match status.shuffle {
        Shuffle::Enabled => "on",
        Shuffle::Disabled => "off",
    };
    let repeat: &'static str = <&'static str>::from(status.repeat_mode);

    let flag = |label: &'static str, value: Cow<'a, str>| -> Vec<StyledText<'a>> {
        vec![text(label).fg(colors.dim), text(value).fg(colors.accent)]
    };
    let flag_separator = || text(glyphs.flag_separator).fg(colors.dim);

    let head = [
        text(glyphs.name).fg(colors.frame),
        text(glyphs.separator).fg(colors.dim),
        text(pos_total).fg(colors.accent),
        text(glyphs.separator).fg(colors.dim),
    ];
    let mut flags: Vec<(&'static str, Cow<'a, str>)> = vec![
        (SHUFFLE_LABEL, Cow::Borrowed(shuffle)),
        (REPEAT_LABEL, Cow::Borrowed(repeat)),
        (QUEUE_LABEL, Cow::Owned(status.queue_len.to_string())),
        (THEME_LABEL, Cow::Borrowed(status.theme_name)),
    ];
    if let Some(sleep_left) = status.sleep_left {
        flags.push((SLEEP_LABEL, Cow::Owned(sleep_label(sleep_left))));
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

    truncate_line_to_width(line(pieces), row_width, TruncateGlyphs::default())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{domain::Shuffle, playlist::RepeatMode};
    use ratatui::style::Color;
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::status_line::{
        ScanProgress,
        StatusLineColors,
        StatusLineView,
        sleep_label,
        status_line,
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
            shuffle: Shuffle::Enabled,
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
        status_line(status, colors(), 80)
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
    fn the_status_line_shows_every_label() {
        let line = status_line(view(), colors(), 80);
        insta::assert_debug_snapshot!(line);
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let text: String = status_line(view(), colors(), 80)
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
        let status = StatusLineView {
            sleep_left: Some(Duration::from_secs(14 * 60 + 59)),
            ..view()
        };
        let text: String = status_line(status, colors(), 100)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.ends_with("sleep 15m"), "got {text:?}");
    }

    #[rstest]
    #[case::rounds_up_from_one_second_left(Duration::from_secs(14 * 60 + 59), "15m")]
    #[case::exact_quarter_hour(Duration::from_secs(15 * 60), "15m")]
    #[case::rounds_up_past_the_quarter_hour(Duration::from_secs(15 * 60 + 1), "16m")]
    #[case::last_minute(Duration::from_secs(1), "1m")]
    #[case::no_time_left(Duration::ZERO, "0m")]
    fn the_sleep_label_rounds_minutes_up(
        #[case] left: Duration,
        #[case] expected: &str,
    ) {
        assert_eq!(sleep_label(left), expected);
    }

    #[test]
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let budget = 24;
        let text: String = status_line(view(), colors(), budget)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.width() <= budget, "got {text:?}");
        assert!(text.ends_with('…'), "got {text:?}");
        assert!(text.starts_with("Playlist"), "got {text:?}");
    }
}
