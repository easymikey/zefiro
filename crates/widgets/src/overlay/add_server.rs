use std::borrow::Cow;

use kernel::domain::{
    geometry::Cells,
    overlay::{Field, ServerPrompt},
    server::ServerStatus,
};
use ratatui::{style::Style, text::Line};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::prompt::{CURSOR, MARKER, PromptBody, PromptWidget, cursor},
    primitive::{
        span::{line, text},
        truncate::{truncate_head, truncate_owned},
    },
    theme::active_theme::ActiveTheme,
};

const SECRET_DOT: &str = "•";
const TITLE: &str = "Add server";
const HINT: &str = "tab next · shift+tab back · enter connect · esc cancel";
const MIN_WIDTH: Cells = Cells(66);
const SELECTED_MARKER: &str = "┃ ";
const UNSELECTED_MARKER: &str = "  ";
const FIELDS: [Field; 3] = [Field::Link, Field::User, Field::Password];

fn label(field: Field) -> &'static str {
    match field {
        Field::Link => "Link",
        Field::User => "User",
        Field::Password => "Password",
    }
}

fn placeholder(field: Field) -> &'static str {
    match field {
        Field::Link => "https://music.example.com",
        Field::User => "admin",
        Field::Password => "••••••••",
    }
}

fn description(field: Field) -> Option<&'static str> {
    match field {
        Field::Link => Some("http(s)://host[:port]"),
        Field::User => None,
        Field::Password => Some("Kept in the macOS Keychain"),
    }
}

fn entered(
    server_prompt: &ServerPrompt,
    field: Field,
    budget: usize,
) -> (Cow<'_, str>, Option<String>) {
    match field {
        Field::Link => (
            truncate_head(&server_prompt.link_text_entry.input, budget),
            server_prompt
                .link_text_entry
                .error
                .as_ref()
                .map(ToString::to_string),
        ),
        Field::User => (
            truncate_head(&server_prompt.user_text_entry.input, budget),
            server_prompt
                .user_text_entry
                .error
                .as_ref()
                .map(ToString::to_string),
        ),
        Field::Password => (
            Cow::Owned(
                SECRET_DOT.repeat(
                    server_prompt
                        .password_text_entry
                        .input
                        .chars()
                        .count()
                        .min(budget),
                ),
            ),
            server_prompt
                .password_text_entry
                .error
                .as_ref()
                .map(ToString::to_string),
        ),
    }
}

fn status_line(
    server_prompt: &ServerPrompt,
    active_theme: ActiveTheme<'_>,
    width: usize,
) -> Line<'static> {
    match &server_prompt.server_status {
        Some(ServerStatus::Connecting) => {
            line([text("Connecting…").fg(active_theme.colors().muted_foreground)])
        }
        Some(ServerStatus::Offline(remote_error)) => {
            line([text(truncate_owned(remote_error.to_string(), width))
                .fg(active_theme.alert())])
        }
        Some(ServerStatus::Online(_)) | None => Line::default(),
    }
}

fn rows<'a>(
    server_prompt: &'a ServerPrompt,
    field: Field,
    active_theme: ActiveTheme<'_>,
) -> impl Iterator<Item = Line<'a>> {
    let colors = active_theme.colors();
    let width = usize::from(MIN_WIDTH.0);
    let budget =
        width.saturating_sub(SELECTED_MARKER.width() + MARKER.width() + CURSOR.width());
    let (entered, error) = entered(server_prompt, field, budget);
    let (glyph, title_color, marker_color, input) = if field == server_prompt.field {
        (
            SELECTED_MARKER,
            colors.accent,
            colors.accent,
            cursor(entered, placeholder(field)),
        )
    } else if entered.is_empty() {
        (
            UNSELECTED_MARKER,
            colors.foreground,
            colors.muted_foreground,
            [text(placeholder(field)).dim(), text("")],
        )
    } else {
        (
            UNSELECTED_MARKER,
            colors.foreground,
            colors.muted_foreground,
            [text(entered), text("")],
        )
    };
    let marker = text(glyph).fg(colors.accent);
    let title = line([marker.clone(), text(label(field)).fg(title_color)]);
    let described = description(field).map(|description| {
        line([
            marker.clone(),
            text(description).fg(colors.muted_foreground),
        ])
    });
    let field_line = line(
        [marker.clone(), text(MARKER).fg(marker_color)]
            .into_iter()
            .chain(input),
    )
    .style(Style::new().fg(colors.foreground));
    let refusal = error.map_or_else(Line::default, |message| {
        line([
            marker,
            text(truncate_owned(
                format!("* {message}"),
                width.saturating_sub(SELECTED_MARKER.width()),
            ))
            .fg(active_theme.alert()),
        ])
    });
    [Some(title), described, Some(field_line), Some(refusal)]
        .into_iter()
        .flatten()
}

#[must_use]
pub(crate) fn prompt<'a>(
    server_prompt: &'a ServerPrompt,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    let width = usize::from(MIN_WIDTH.0);
    let lines = FIELDS.into_iter().flat_map(|field| {
        (field != Field::Link)
            .then(Line::default)
            .into_iter()
            .chain(rows(server_prompt, field, active_theme))
    });
    PromptWidget::new(
        PromptBody::Form(
            lines
                .chain([status_line(server_prompt, active_theme, width)])
                .collect(),
        ),
        active_theme,
    )
    .title(TITLE)
    .hint(HINT)
    .min_width(MIN_WIDTH)
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        io_error::IoError,
        overlay::{Field, ServerPrompt, TextEntry},
        server::{EndpointError, RemoteError, ServerName, ServerStatus, UserNameError},
    };
    use ratatui::{backend::TestBackend, layout::Rect, style::Modifier};

    use crate::{
        overlay::add_server::prompt,
        primitive::canvas::Canvas,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn entry<E>(input: &str, error: Option<E>) -> TextEntry<E> {
        TextEntry {
            input: input.to_string(),
            error,
        }
    }

    fn filled() -> ServerPrompt {
        ServerPrompt {
            link_text_entry: entry("https://music.example.com", None),
            user_text_entry: entry("alice", None),
            password_text_entry: entry("hunter2", None),
            field: Field::Password,
            ..ServerPrompt::default()
        }
    }

    fn refused() -> ServerPrompt {
        ServerPrompt {
            link_text_entry: entry("https://music.example.com", None),
            user_text_entry: entry("", Some(UserNameError::Empty)),
            field: Field::Link,
            server_status: Some(ServerStatus::Offline(RemoteError::Unreachable {
                server_name: ServerName::new("music.example.com"),
                source: IoError::Other,
            })),
            ..ServerPrompt::default()
        }
    }

    fn outer(server_prompt: &ServerPrompt) -> Rect {
        let theme = noir();
        prompt(
            server_prompt,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .areas(Rect::new(0, 0, 100, 30), &[])
        .outer
    }

    fn backend_at(width: u16, server_prompt: &ServerPrompt) -> TestBackend {
        let theme = noir();
        let widget = prompt(
            server_prompt,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        rendered(width, 30, |frame| {
            let area = frame.area();
            widget.paint(
                widget.areas(area, &[]),
                Canvas {
                    area,
                    buffer: frame.buffer_mut(),
                },
            );
        })
    }

    fn frame_at(width: u16, server_prompt: &ServerPrompt) -> String {
        backend_at(width, server_prompt).to_string()
    }

    #[test]
    fn add_server_form_opens_empty_with_every_hint_inside_its_field_at_80_columns() {
        let screen = frame_at(80, &ServerPrompt::default());
        for hint in [
            "┃ Link",
            "┃ http(s)://host[:port]",
            "┃ > https://music.example.com",
            "  > admin",
            "  Kept in the macOS Keychain",
            "  > ••••••••",
            "tab next · shift+tab back · enter connect · esc cancel",
        ] {
            assert!(screen.contains(hint), "{hint}");
        }
        assert!(!screen.contains(&EndpointError::Empty.to_string()));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn the_focused_empty_field_puts_a_block_cursor_on_the_first_character_of_its_placeholder()
     {
        let backend = backend_at(100, &ServerPrompt::default());
        let buffer = backend.buffer();
        let area = buffer.area;
        let (x, y) = (area.top()..area.bottom())
            .find_map(|y| {
                let symbols: Vec<&str> = (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                symbols
                    .windows(3)
                    .position(|cells| cells == [">", " ", "h"])
                    .map(|start| (u16::try_from(start + 2).unwrap(), y))
            })
            .unwrap();
        assert_eq!(
            (
                buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                buffer[(x + 1, y)].modifier.contains(Modifier::REVERSED)
            ),
            (true, false)
        );
    }

    #[test]
    fn add_server_form_shows_an_error_as_a_starred_line_under_its_field() {
        let screen = frame_at(100, &refused());
        assert!(screen.contains(&format!("  * {}", UserNameError::Empty)));
    }

    #[test]
    fn add_server_form_shows_every_filled_field_and_a_dot_per_password_character() {
        let screen = frame_at(100, &filled());
        assert!(!screen.contains("hunter2"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn add_server_form_shows_a_field_error_under_its_field_and_the_refusal_under_the_form()
     {
        insta::assert_snapshot!(frame_at(100, &refused()));
    }

    #[test]
    fn add_server_form_keeps_one_size_from_open_to_refusal() {
        let opened = outer(&ServerPrompt::default());
        assert_eq!([outer(&filled()), outer(&refused())], [opened, opened]);
    }
}
