use std::{borrow::Cow, time::Duration};

use kernel::domain::{
    catalog::CatalogName,
    geometry::Cells,
    index::ViewIndex,
    model::ScanStatus,
    playlist::{PlaylistSource, RepeatMode},
    server::{Server, ServerStatus},
    startup::Shuffle,
    time::Moment,
};
use ratatui::{style::Color, text::Line};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        glyphs::{CREDENTIALS_GLYPH, DOT_SEPARATOR, OFFLINE_GLYPH, TITLE_SEPARATOR},
        span::{StyledText, line, text},
        spinner::Spinner,
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
    pub(crate) spinner: Spinner,
    pub(crate) theme_name: &'a str,
    pub(crate) remaining: Option<Duration>,
    pub(crate) servers: &'a [Server],
    pub(crate) catalog_name: &'a CatalogName,
    pub(crate) playlist_source: &'a PlaylistSource,
}

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

fn glyph(server_status: &ServerStatus, spinner: Spinner) -> Option<&'static str> {
    match server_status {
        ServerStatus::Connecting => Some(spinner.glyph()),
        ServerStatus::Online(_session) => None,
        ServerStatus::Offline(remote_error) => Some(if remote_error.is_credentials() {
            CREDENTIALS_GLYPH
        } else {
            OFFLINE_GLYPH
        }),
    }
}

fn chip(server: &Server, spinner: Spinner) -> impl Iterator<Item = &str> {
    let state = glyph(&server.server_status, spinner);
    [
        Some(server.account.server_name.as_str()),
        state.and(Some(" ")),
        state,
    ]
    .into_iter()
    .flatten()
}

fn tone(
    status_line_view: StatusLineView<'_>,
    server: Option<&Server>,
    colors: &Colors<Color>,
) -> Color {
    match (status_line_view.catalog_name, server) {
        (CatalogName::Server(server_name), Some(server))
            if *server_name == server.account.server_name =>
        {
            colors.accent
        }
        (CatalogName::Local, None) if !status_line_view.servers.is_empty() => {
            colors.accent
        }
        (CatalogName::Server(_) | CatalogName::Local, Some(_) | None) => {
            colors.muted_foreground
        }
    }
}

fn sleep_label(remaining: Duration) -> String {
    format!("{}m", ceil_minutes(remaining))
}

fn chips<'a>(
    status_line_view: StatusLineView<'a>,
    colors: &Colors<Color>,
    room: usize,
) -> impl Iterator<Item = StyledText<'a>> {
    let spinner = status_line_view.spinner;
    status_line_view
        .servers
        .iter()
        .scan(0, move |used, server| {
            *used += DOT_SEPARATOR.width()
                + chip(server, spinner)
                    .map(UnicodeWidthStr::width)
                    .sum::<usize>();
            (*used <= room).then_some(server)
        })
        .flat_map(move |server| {
            let tone = tone(status_line_view, Some(server), colors);
            [text(DOT_SEPARATOR).fg(colors.muted_foreground)]
                .into_iter()
                .chain(
                    chip(server, spinner)
                        .zip([
                            tone,
                            tone,
                            match server.server_status {
                                ServerStatus::Connecting => colors.accent,
                                ServerStatus::Online(_) | ServerStatus::Offline(_) => {
                                    tone
                                }
                            },
                        ])
                        .map(|(piece, color)| text(piece).fg(color)),
                )
        })
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
    let flag_separator = || text(DOT_SEPARATOR).fg(colors.muted_foreground);
    let spinner = status_line_view.spinner;
    let mark = match status_line_view.scan_status {
        ScanStatus::Idle => None,
        ScanStatus::Scanning | ScanStatus::Tagging { .. } => Some(spinner.mark(colors)),
    };
    let name = match status_line_view.playlist_source {
        PlaylistSource::Named => "Playlist",
        PlaylistSource::Library
        | PlaylistSource::Server(_)
        | PlaylistSource::Songs(_) => "Library",
    };
    let lead_width = 2 * usize::from(mark.is_some());
    let room = row_width.count().saturating_sub(
        name.width() + 2 * TITLE_SEPARATOR.width() + lead_width + pos_total.width(),
    );

    let head = [text(name).fg(tone(status_line_view, None, colors))]
        .into_iter()
        .chain(chips(status_line_view, colors, room))
        .chain([text(TITLE_SEPARATOR).fg(colors.muted_foreground)])
        .chain(mark.into_iter().flatten())
        .chain([
            text(pos_total).fg(colors.accent),
            text(TITLE_SEPARATOR).fg(colors.muted_foreground),
        ]);
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
    let pieces = head.chain(flags.into_iter().chain(sleep).enumerate().flat_map(
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
        catalog::CatalogName,
        geometry::Cells,
        index::ViewIndex,
        model::ScanStatus,
        playlist::RepeatMode,
        server::{
            Account,
            ApiCode,
            Endpoint,
            RemoteError,
            Server,
            ServerName,
            ServerStatus,
            Session,
            UserName,
        },
        startup::Shuffle,
        time::Moment,
    };
    use ratatui::{layout::Rect, style::Color};
    use rstest::rstest;

    use crate::{
        playlist::chrome::pane_title,
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
            spinner: crate::primitive::spinner::Spinner::default(),
            theme_name: "rose-pine",
            remaining: None,
            servers: &[],
            catalog_name: &CatalogName::Local,
            playlist_source: &kernel::domain::playlist::PlaylistSource::Named,
        }
    }

    #[test]
    fn the_local_tab_is_titled_library_while_a_server_track_plays() {
        let playlist_source =
            kernel::domain::playlist::PlaylistSource::Songs(ServerName::new("home"));
        let status_line_view = StatusLineView {
            playlist_source: &playlist_source,
            ..view()
        };
        insta::assert_snapshot!(
            status_line(status_line_view, &colors(), Cells(80)).to_string()
        );
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
    #[case::rounds_up_past_the_quarter_hour(Duration::from_secs(15 * 60 + 1), "16m")]
    #[case::no_time_left(Duration::ZERO, "0m")]
    fn the_sleep_label_rounds_minutes_up(
        #[case] remaining: Duration,
        #[case] expected: &str,
    ) {
        assert_eq!(sleep_label(remaining), expected);
    }

    #[rstest]
    #[case::a_shown_label_wakes_at_the_next_minute(
        Some(Duration::from_secs(14 * 60 + 59)),
        Presence::Shown,
        Some(Duration::from_secs(59))
    )]
    #[case::a_hidden_label_wants_no_frame(
        Some(Duration::from_secs(60)),
        Presence::Hidden,
        None
    )]
    #[case::no_deadline_wants_no_frame(None, Presence::Shown, None)]
    fn a_sleep_timer_wakes_once_a_minute(
        #[case] remaining: Option<Duration>,
        #[case] label: Presence,
        #[case] until_next: Option<Duration>,
    ) {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline_at = remaining.map(|left| Moment::new(now.since_epoch() + left));
        assert_eq!(
            sleep_frame_due(deadline_at, label, now),
            until_next.map(|left| Moment::new(now.since_epoch() + left))
        );
    }

    fn server(name: &str, server_status: ServerStatus) -> Server {
        Server {
            account: Account {
                server_name: ServerName::new(name),
                endpoint: Endpoint::parse("https://music.example").unwrap(),
                user_name: UserName::new("mikey").unwrap(),
            },
            server_status,
        }
    }

    fn three_servers() -> [Server; 3] {
        [
            server("living-room", ServerStatus::Connecting),
            server(
                "office",
                ServerStatus::Offline(RemoteError::Moved {
                    server_name: ServerName::new("office"),
                }),
            ),
            server(
                "studio",
                ServerStatus::Offline(RemoteError::Api {
                    server_name: ServerName::new("studio"),
                    api_code: ApiCode(40),
                }),
            ),
        ]
    }

    #[test]
    fn one_online_server_follows_an_accent_playlist_chip() {
        let servers = [server(
            "home",
            ServerStatus::Online(Session::new(
                Endpoint::parse("https://music.example").unwrap(),
                "u=mikey",
            )),
        )];
        let status_line_view = StatusLineView {
            servers: &servers,
            ..view()
        };
        let line = status_line(status_line_view, &colors(), Cells(100));
        assert_eq!(
            line.to_string(),
            "Playlist · home ── 3/12 ── shuffle on · repeat all · queue 7 · theme rose-pine"
        );
        insta::assert_debug_snapshot!(line);
    }

    #[test]
    fn a_server_tab_moves_the_accent_to_its_chip() {
        let servers = [server(
            "home",
            ServerStatus::Online(Session::new(
                Endpoint::parse("https://music.example").unwrap(),
                "u=mikey",
            )),
        )];
        let catalog_name = CatalogName::Server(ServerName::new("home"));
        let status_line_view = StatusLineView {
            servers: &servers,
            catalog_name: &catalog_name,
            playlist_source: &kernel::domain::playlist::PlaylistSource::Named,
            ..view()
        };
        let line = status_line(status_line_view, &colors(), Cells(100));
        let tone = |content: &str| {
            line.spans
                .iter()
                .find(|span| span.content == content)
                .and_then(|span| span.style.fg)
        };
        assert_eq!(tone("home"), Some(colors().accent));
        assert_eq!(tone("Playlist"), Some(colors().muted_foreground));
    }

    #[test]
    fn each_server_chip_wears_the_glyph_of_its_state() {
        let servers = three_servers();
        let status_line_view = StatusLineView {
            servers: &servers,
            ..view()
        };
        insta::assert_snapshot!(
            status_line(status_line_view, &colors(), Cells(120)).to_string()
        );
    }

    #[test]
    fn a_60_column_pane_drops_server_chips_before_the_counts() {
        let servers = three_servers();
        let theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let status_line_view = StatusLineView {
            servers: &servers,
            ..view()
        };
        let title =
            pane_title(Rect::new(0, 0, 60, 1), status_line_view, &theme).to_string();
        assert!(title.contains("office ○ ── 3/12 ── "), "got {title:?}");
        assert!(!title.contains("studio"), "got {title:?}");
        insta::assert_snapshot!(title);
    }
}
