use std::{borrow::Cow, time::Duration};

use kernel::domain::{
    geometry::Cells,
    index::ViewIndex,
    model::ScanStatus,
    playlist::RepeatMode,
    startup::Shuffle,
    time::Moment,
};
use ratatui::{style::Color, text::Line};

use crate::{
    primitive::{
        span::{StyledText, line, text},
        truncate::truncate_line,
    },
    repaint::{Presence, ceil_minutes, next_sleep_minute},
    theme::colors::Colors,
};

const SHUFFLE_LABEL: &str = "shuffle ";
const REPEAT_LABEL: &str = "repeat ";
const QUEUE_LABEL: &str = "queue ";
const THEME_LABEL: &str = "theme ";
const SLEEP_LABEL: &str = "sleep ";

#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusLineView<'a> {
    pub(crate) shuffle: Shuffle,
    pub(crate) repeat_mode: RepeatMode,
    pub(crate) queue_len: usize,
    pub(crate) selected: ViewIndex,
    pub(crate) playlist_len: usize,
    pub(crate) scan_status: ScanStatus,
    pub(crate) scanning_label: &'a str,
    pub(crate) theme_name: &'a str,
    pub(crate) remaining: Option<Duration>,
}

const NAME: &str = "Playlist";

fn counts<'a>(status_line_view: StatusLineView<'a>) -> Cow<'a, str> {
    match status_line_view.scan_status {
        ScanStatus::Idle => Cow::Owned(format!(
            "{}/{}",
            (status_line_view.selected.get() + 1).min(status_line_view.playlist_len),
            status_line_view.playlist_len
        )),
        ScanStatus::Scanning => Cow::Borrowed(status_line_view.scanning_label),
        ScanStatus::Tagging { done, total } => {
            Cow::Owned(format!("{total} tracks · tagging {done}/{total}"))
        }
    }
}

fn sleep_label(remaining: Duration) -> String {
    format!("{}m", ceil_minutes(remaining))
}

#[must_use]
pub(crate) fn status_line<'a>(
    status_line_view: StatusLineView<'a>,
    colors: &Colors<Color>,
    row_width: Cells,
) -> Line<'a> {
    let pos_total = counts(status_line_view);

    let shuffle: &'static str = match status_line_view.shuffle {
        Shuffle::On => "on",
        Shuffle::Off => "off",
    };
    let repeat: &'static str = <&'static str>::from(status_line_view.repeat_mode);

    let flag = |label: &'static str, value: Cow<'a, str>| -> [StyledText<'a>; 2] {
        [
            text(label).fg(colors.muted_foreground),
            text(value).fg(colors.accent),
        ]
    };
    let flag_separator =
        || text(crate::primitive::glyphs::DOT_SEPARATOR).fg(colors.muted_foreground);

    let head = [
        text(NAME).fg(colors.muted_foreground),
        text(crate::primitive::glyphs::TITLE_SEPARATOR).fg(colors.muted_foreground),
        text(pos_total).fg(colors.accent),
        text(crate::primitive::glyphs::TITLE_SEPARATOR).fg(colors.muted_foreground),
    ];
    let flags: [(&'static str, Cow<'a, str>); 4] = [
        (SHUFFLE_LABEL, Cow::Borrowed(shuffle)),
        (REPEAT_LABEL, Cow::Borrowed(repeat)),
        (
            QUEUE_LABEL,
            Cow::Owned(status_line_view.queue_len.to_string()),
        ),
        (THEME_LABEL, Cow::Borrowed(status_line_view.theme_name)),
    ];
    let sleep = status_line_view
        .remaining
        .map(|sleep_left| (SLEEP_LABEL, Cow::Owned(sleep_label(sleep_left))));
    let pieces =
        head.into_iter()
            .chain(flags.into_iter().chain(sleep).enumerate().flat_map(
                |(index, (label, value))| {
                    (index > 0)
                        .then(flag_separator)
                        .into_iter()
                        .chain(flag(label, value))
                },
            ));

    truncate_line(line(pieces), row_width.count())
}

#[must_use]
pub fn sleep_frame_due(
    deadline_at: Option<Moment>,
    label: Presence,
    now: Moment,
) -> Option<Moment> {
    if label != Presence::Shown {
        return None;
    }
    next_sleep_minute(deadline_at?, now)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{
        geometry::Cells,
        index::ViewIndex,
        model::ScanStatus,
        playlist::RepeatMode,
        startup::Shuffle,
        time::Moment,
    };
    use ratatui::style::Color;
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::{
        repaint::Presence,
        status_line::{StatusLineView, sleep_frame_due, sleep_label, status_line},
        test_support::noir,
        theme::{active_theme::ActiveTheme, colors::Colors, rgb::ColorDepth},
    };

    fn colors() -> Colors<Color> {
        ActiveTheme::new(&noir(), ColorDepth::TrueColor).colors()
    }

    fn view() -> StatusLineView<'static> {
        StatusLineView {
            shuffle: Shuffle::On,
            repeat_mode: RepeatMode::All,
            queue_len: 7,
            selected: ViewIndex::new(2),
            playlist_len: 12,
            scan_status: ScanStatus::Idle,
            scanning_label: "Scanning…",
            theme_name: "rose-pine",
            remaining: None,
        }
    }

    #[test]
    fn tagging_counts_the_tracks_whose_tags_are_already_read() {
        let status_line_view = StatusLineView {
            scan_status: ScanStatus::Tagging {
                done: 64,
                total: 128,
            },
            ..view()
        };
        insta::assert_snapshot!(
            status_line(status_line_view, &colors(), Cells(80)).to_string()
        );
    }

    #[test]
    fn a_scan_in_flight_wears_the_theme_word() {
        let status_line_view = StatusLineView {
            scan_status: ScanStatus::Scanning,
            ..view()
        };
        insta::assert_snapshot!(
            status_line(status_line_view, &colors(), Cells(80)).to_string()
        );
    }

    #[test]
    fn the_status_line_shows_every_label() {
        let colors = Colors {
            muted_foreground: Color::Gray,
            accent: Color::Cyan,
            ..Colors::default()
        };
        let line = status_line(view(), &colors, Cells(80));
        insta::assert_debug_snapshot!(line);
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let text: String = status_line(view(), &colors(), Cells(80))
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
        let status_line_view = StatusLineView {
            remaining: Some(Duration::from_secs(14 * 60 + 59)),
            ..view()
        };
        let text: String = status_line(status_line_view, &colors(), Cells(100))
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
        #[case] remaining: Duration,
        #[case] expected: &str,
    ) {
        assert_eq!(sleep_label(remaining), expected);
    }

    #[test]
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let budget = Cells(24);
        let text: String = status_line(view(), &colors(), budget)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.width() <= budget.count(), "got {text:?}");
        assert!(text.ends_with('…'), "got {text:?}");
        assert!(text.starts_with("Playlist"), "got {text:?}");
    }

    #[test]
    fn a_sleep_timer_wakes_once_a_minute() {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline_at =
            Moment::new(now.since_epoch() + Duration::from_secs(14 * 60 + 59));

        assert_eq!(
            sleep_frame_due(Some(deadline_at), Presence::Shown, now),
            Some(Moment::new(now.since_epoch() + Duration::from_secs(59)))
        );
    }

    #[test]
    fn a_hidden_sleep_label_wants_no_frame() {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline_at = Moment::new(now.since_epoch() + Duration::from_secs(60));

        assert_eq!(
            sleep_frame_due(Some(deadline_at), Presence::Hidden, now),
            None
        );
    }

    #[test]
    fn no_deadline_wants_no_sleep_frame() {
        let now = Moment::new(Duration::from_secs(1_000));

        assert_eq!(sleep_frame_due(None, Presence::Shown, now), None);
    }
}
