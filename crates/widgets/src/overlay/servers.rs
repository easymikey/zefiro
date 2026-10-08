use kernel::domain::{
    geometry::Cells,
    index::ViewIndex,
    server::{Server, ServerStatus},
};
use ratatui::{layout::Rect, text::Line};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalSize},
    primitive::{
        canvas::Canvas,
        span::{StyledText, line, text},
    },
    theme::active_theme::ActiveTheme,
};

const TITLE: &str = "Servers";
const HINT: &str = "Enter edit · t reconnect · d remove · u add";
const EMPTY_PLACEHOLDER: &str = "No servers · u adds one";
const MARKER: &str = "> ";
const MIN_WIDTH: Cells = Cells(44);

#[derive(Debug)]
pub(crate) struct ServersWidget<'a> {
    rows: Vec<Line<'a>>,
    avoid: &'a [Rect],
    theme: ActiveTheme<'a>,
}

fn status(
    server_status: &ServerStatus,
    active_theme: &ActiveTheme<'_>,
) -> StyledText<'static> {
    let colors = active_theme.colors();
    match server_status {
        ServerStatus::Connecting => text("connecting…").fg(colors.muted_foreground),
        ServerStatus::Online(_) => text("online").fg(colors.foreground),
        ServerStatus::Offline(error) if error.is_credentials() => {
            text("wrong user or password").fg(active_theme.alert())
        }
        ServerStatus::Offline(error) => {
            text(format!("offline: {error}")).fg(active_theme.alert())
        }
    }
}

impl<'a> ServersWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        servers: &'a [Server],
        selected: ViewIndex,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        let colors = active_theme.colors();
        let columns = |server: &'a Server| {
            [
                server.account.server_name.as_str(),
                server.account.endpoint.host(),
                server.account.user_name.as_str(),
            ]
        };
        let widths = servers.iter().map(columns).fold(
            [0; 3],
            |[name, host, user], [name_column, host_column, user_column]| {
                [
                    name.max(name_column.width()),
                    host.max(host_column.width()),
                    user.max(user_column.width()),
                ]
            },
        );
        let rows =
            servers
                .iter()
                .enumerate()
                .map(|(index, server)| {
                    let chosen = index == selected.get();
                    let marker = if chosen { MARKER } else { "  " };
                    let padded = columns(server).into_iter().zip(widths).map(
                        |(column, width)| {
                            text(format!(
                                "{column}{}  ",
                                " ".repeat(width.saturating_sub(column.width()))
                            ))
                            .fg(colors.foreground)
                        },
                    );
                    let pieces = [text(marker).fg(colors.accent)]
                        .into_iter()
                        .chain(padded)
                        .chain([status(&server.server_status, &active_theme)]);
                    line(pieces.map(|piece| {
                        if chosen {
                            piece
                                .fg(colors.selection_foreground)
                                .bg(colors.selection_background)
                        } else {
                            piece
                        }
                    }))
                })
                .collect::<Vec<_>>();
        let rows = if rows.is_empty() {
            vec![line([text(EMPTY_PLACEHOLDER).fg(colors.muted_foreground)])]
        } else {
            rows
        };
        Self {
            rows,
            avoid: &[],
            theme: active_theme,
        }
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }

    fn modal(&self) -> Modal<'_> {
        let colors = self.theme.colors();
        let widest = self
            .rows
            .iter()
            .map(Line::width)
            .fold(TITLE.width(), usize::max);
        Modal {
            title: TITLE,
            size: ModalSize::Dialog {
                min_width: MIN_WIDTH,
                content_width: MIN_WIDTH
                    .max(u16::try_from(widest).map_or(MIN_WIDTH, Cells)),
                content_rows: u16::try_from(self.rows.len())
                    .map_or(Cells(u16::MAX), Cells),
            },
            hint: Some(line([text(HINT).fg(colors.muted_foreground)])),
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalAreas {
        self.modal().areas(screen, self.avoid)
    }

    pub(crate) fn paint(&self, areas: ModalAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        self.modal().paint(areas, buffer);
        let body = areas.body;
        for (row, y) in self.rows.iter().zip(body.y..body.bottom()) {
            buffer.set_line(body.x, y, row, body.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        index::ViewIndex,
        io_error::IoError,
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
    };

    use crate::{
        overlay::servers::ServersWidget,
        primitive::canvas::Canvas,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn account(name: &str, link: &str, user: &str) -> Account {
        Account {
            server_name: ServerName::new(name),
            endpoint: Endpoint::parse(link).unwrap(),
            user_name: UserName::new(user).unwrap(),
        }
    }

    fn frame(servers: &[Server]) -> String {
        let theme = noir();
        let widget = ServersWidget::new(
            servers,
            ViewIndex::new(1),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        rendered(100, 20, |frame| {
            let area = frame.area();
            widget.paint(
                widget.areas(area),
                Canvas {
                    area,
                    buffer: frame.buffer_mut(),
                },
            );
        })
        .to_string()
    }

    #[test]
    fn servers_overlay_without_servers_says_u_adds_one() {
        insta::assert_snapshot!(frame(&[]));
    }

    #[test]
    fn servers_overlay_shows_each_state_and_marks_the_selected_row() {
        let session = Session::new(
            Endpoint::parse("https://music.example.com").unwrap(),
            "u=alice&t=token&s=salt",
        );
        let servers = [
            Server {
                account: account("home", "https://music.example.com", "alice"),
                server_status: ServerStatus::Online(session),
            },
            Server {
                account: account("office", "https://tunes.example.org", "bob"),
                server_status: ServerStatus::Connecting,
            },
            Server {
                account: account("attic", "http://10.0.0.2:4533", "carol"),
                server_status: ServerStatus::Offline(RemoteError::Unreachable {
                    server_name: ServerName::new("attic"),
                    source: IoError::Other,
                }),
            },
            Server {
                account: account("lab", "https://lab.example.net", "dave"),
                server_status: ServerStatus::Offline(RemoteError::Api {
                    server_name: ServerName::new("lab"),
                    api_code: ApiCode(40),
                }),
            },
        ];
        insta::assert_snapshot!(frame(&servers));
    }
}
