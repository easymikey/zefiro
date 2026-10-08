use kernel::domain::{
    geometry::Cells,
    index::ViewIndex,
    server::{Server, ServerStatus},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Span};
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

#[derive(Debug, Clone, PartialEq)]
pub struct ServersTable<'a> {
    rows: Vec<[Span<'a>; 5]>,
    starts: [usize; 5],
    content_width: Cells,
}

#[derive(Debug)]
pub(crate) struct ServersWidget<'a> {
    servers_table: &'a ServersTable<'a>,
    selected: ViewIndex,
    avoid: &'a [Rect],
    theme: ActiveTheme<'a>,
}

fn columns(server: &Server) -> [&str; 3] {
    [
        server.account.server_name.as_str(),
        server.account.endpoint.host(),
        server.account.user_name.as_str(),
    ]
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

impl<'a> ServersTable<'a> {
    #[must_use]
    pub(crate) fn new(
        servers: &'a [Server],
        selected: ViewIndex,
        active_theme: &ActiveTheme<'_>,
    ) -> Self {
        let colors = active_theme.colors();
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
        let starts: [usize; 5] = std::array::from_fn(|column| {
            widths
                .iter()
                .take(column.saturating_sub(1))
                .map(|width| width + 2)
                .sum::<usize>()
                + usize::from(column != 0) * MARKER.width()
        });
        let rows = servers
            .iter()
            .enumerate()
            .map(|(index, server)| {
                let marker = if index == selected.get() {
                    MARKER
                } else {
                    "  "
                };
                let [name, host, user] = columns(server).map(|column| {
                    Span::styled(column, Style::new().fg(colors.foreground))
                });
                [
                    Span::styled(marker, Style::new().fg(colors.accent)),
                    name,
                    host,
                    user,
                    status(&server.server_status, active_theme).into(),
                ]
            })
            .collect::<Vec<_>>();
        let [.., start] = starts;
        let widest = rows
            .iter()
            .map(|[.., status]| start + status.width())
            .max()
            .unwrap_or(EMPTY_PLACEHOLDER.width())
            .max(TITLE.width());
        Self {
            rows,
            starts,
            content_width: MIN_WIDTH
                .max(u16::try_from(widest).map_or(MIN_WIDTH, Cells)),
        }
    }
}

impl<'a> ServersWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        servers_table: &'a ServersTable<'a>,
        selected: ViewIndex,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            servers_table,
            selected,
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
        Modal {
            title: TITLE,
            size: ModalSize::Dialog {
                min_width: MIN_WIDTH,
                content_width: self.servers_table.content_width,
                content_rows: u16::try_from(self.servers_table.rows.len().max(1))
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
        if self.servers_table.rows.is_empty() && body.height != 0 {
            buffer.set_stringn(
                body.x,
                body.y,
                EMPTY_PLACEHOLDER,
                usize::from(body.width),
                Style::new().fg(self.theme.colors().muted_foreground),
            );
        }
        self.paint_rows(body, buffer);
    }

    fn paint_rows(&self, body: Rect, buffer: &mut Buffer) {
        let colors = self.theme.colors();
        let starts = self.servers_table.starts;
        let place = |start: usize| {
            u16::try_from(start)
                .ok()
                .map(|start| body.x.saturating_add(start))
                .filter(|x| *x < body.right())
        };
        for (index, (row, y)) in self
            .servers_table
            .rows
            .iter()
            .zip(body.y..body.bottom())
            .enumerate()
        {
            for (span, start) in row.iter().zip(starts) {
                if let Some(x) = place(start) {
                    buffer.set_span(x, y, span, body.right() - x);
                }
            }
            if index == self.selected.get() {
                let [.., status] = row;
                let [.., start] = starts;
                let row_width = u16::try_from(start + status.width())
                    .map_or(body.width, |width| width.min(body.width));
                buffer.set_style(
                    Rect {
                        y,
                        width: row_width,
                        height: 1,
                        ..body
                    },
                    Style::new()
                        .fg(colors.selection_foreground)
                        .bg(colors.selection_background),
                );
            }
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
        overlay::servers::{ServersTable, ServersWidget},
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
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let selected = ViewIndex::new(1);
        let servers_table = ServersTable::new(servers, selected, &active_theme);
        let widget = ServersWidget::new(&servers_table, selected, active_theme);
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
