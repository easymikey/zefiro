use std::{borrow::Cow, time::Duration};

use kernel::{
    domain::{ScanStatus, Shuffle, ViewIndex, geometry::Cells},
    playlist::RepeatMode,
};
use ratatui::{style::Color, text::Line};

use crate::{
    primitive::{
        span::{StyledText, line, text},
        text::truncate_line_to_width,
    },
    repaint::ceil_minutes,
    scene::Scene,
    theme::{ActiveTheme, Role},
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
    pub(crate) position: ViewIndex,
    pub(crate) total: usize,
    pub(crate) scan: ScanProgress<'a>,
    pub(crate) theme_name: &'a str,
    pub(crate) sleep_left: Option<Duration>,
}

impl<'a> StatusLineView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let shuffle = if scene.playlist.play_order.is_shuffle() {
            Shuffle::Enabled
        } else {
            Shuffle::Disabled
        };
        Self {
            shuffle,
            repeat_mode: scene.playlist.repeat,
            queue_len: scene.queue.len(),
            position: scene.browse_selected,
            total: scene.playlist.tracks.len(),
            scan: ScanProgress::of(scene.scan, scene.theme.scanning_label.as_str()),
            theme_name: scene.theme.name.as_str(),
            sleep_left: scene.sleep_left,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusLineStyle {
    pub(crate) border: Color,
    pub(crate) muted_foreground: Color,
    pub(crate) accent: Color,
}

impl StatusLineStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            border: theme.role(Role::Frame),
            muted_foreground: theme.role(Role::Dim),
            accent: theme.role(Role::Accent),
        }
    }
}

const NAME: &str = "Playlist";
const FLAG_SEPARATOR: &str = " · ";

fn counts(status: StatusLineView<'_>) -> String {
    match status.scan {
        ScanProgress::Done => format!(
            "{}/{}",
            (status.position.get() + 1).min(status.total),
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
    style: StatusLineStyle,
    row_width: Cells,
) -> Line<'a> {
    let pos_total = counts(status);

    let shuffle: &'static str = match status.shuffle {
        Shuffle::Enabled => "on",
        Shuffle::Disabled => "off",
    };
    let repeat: &'static str = <&'static str>::from(status.repeat_mode);

    let flag = |label: &'static str, value: Cow<'a, str>| -> Vec<StyledText<'a>> {
        vec![
            text(label).fg(style.muted_foreground),
            text(value).fg(style.accent),
        ]
    };
    let flag_separator = || text(FLAG_SEPARATOR).fg(style.muted_foreground);

    let head = [
        text(NAME).fg(style.border),
        text(crate::primitive::glyphs::TITLE_SEPARATOR).fg(style.muted_foreground),
        text(pos_total).fg(style.accent),
        text(crate::primitive::glyphs::TITLE_SEPARATOR).fg(style.muted_foreground),
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

    truncate_line_to_width(line(pieces), row_width.count())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        domain::{Shuffle, ViewIndex, geometry::Cells},
        playlist::RepeatMode,
    };
    use ratatui::style::Color;
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::{
        status_line::{
            ScanProgress,
            StatusLineStyle,
            StatusLineView,
            sleep_label,
            status_line,
        },
        test_support::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn colors() -> StatusLineStyle {
        StatusLineStyle::from_theme(&ActiveTheme::new(&noir(), ColorDepth::TrueColor))
    }

    fn view() -> StatusLineView<'static> {
        StatusLineView {
            shuffle: Shuffle::Enabled,
            repeat_mode: RepeatMode::All,
            queue_len: 7,
            position: ViewIndex::new(2),
            total: 12,
            scan: ScanProgress::Done,
            theme_name: "rose-pine",
            sleep_left: None,
        }
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
        insta::assert_snapshot!(status_line(status, colors(), Cells(80)).to_string());
    }

    #[test]
    fn a_scan_in_flight_wears_the_theme_word() {
        let status = StatusLineView {
            scan: ScanProgress::Scanning("Scanning…"),
            ..view()
        };
        insta::assert_snapshot!(status_line(status, colors(), Cells(80)).to_string());
    }

    #[test]
    fn the_status_line_shows_every_label() {
        let style = StatusLineStyle {
            border: Color::Blue,
            muted_foreground: Color::Gray,
            accent: Color::Cyan,
        };
        let line = status_line(view(), style, Cells(80));
        insta::assert_debug_snapshot!(line);
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let text: String = status_line(view(), colors(), Cells(80))
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
        let text: String = status_line(status, colors(), Cells(100))
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
        let budget = Cells(24);
        let text: String = status_line(view(), colors(), budget)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.width() <= budget.count(), "got {text:?}");
        assert!(text.ends_with('…'), "got {text:?}");
        assert!(text.starts_with("Playlist"), "got {text:?}");
    }
}
