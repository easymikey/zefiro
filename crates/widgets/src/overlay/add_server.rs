use std::borrow::Cow;

use kernel::domain::{geometry::Cells, overlay::ServerPrompt};
use ratatui::text::Line;

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::span::{line, text},
    theme::active_theme::ActiveTheme,
};

const SECRET_DOT: &str = "•";
const TITLE: &str = "Add server";
const HINT: &str = "Enter next · Esc cancel";
const MIN_WIDTH: Cells = Cells(40);
const LINK_HINT: &str = "http(s)://host[:port], like https://music.example.com";
const USER_HINT: &str = "The user name on that server";
const PASSWORD_HINT: &str = "Kept in the macOS Keychain once the server accepts it";

fn answer<'a>(
    label: &'static str,
    answer: &'a str,
    active_theme: ActiveTheme<'_>,
) -> Line<'a> {
    let colors = active_theme.colors();
    line([
        text(label).fg(colors.muted_foreground),
        text(answer).fg(colors.foreground),
    ])
}

#[must_use]
pub(crate) fn prompt<'a>(
    server_prompt: &'a ServerPrompt,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    let widget = |field: Cow<'a, str>| {
        PromptWidget::new(PromptBody::Entry(field), active_theme)
            .title(TITLE)
            .hint(HINT)
            .min_width(MIN_WIDTH)
    };
    match server_prompt {
        ServerPrompt::Link { text_entry, .. } => {
            widget(Cow::Borrowed(text_entry.input.as_str()))
                .field_hint(LINK_HINT)
                .error(text_entry.error.as_ref())
        }
        ServerPrompt::User {
            endpoint,
            text_entry,
            ..
        } => widget(Cow::Borrowed(text_entry.input.as_str()))
            .answers(vec![answer("Link  ", endpoint.as_str(), active_theme)])
            .field_hint(USER_HINT)
            .error(text_entry.error.as_ref()),
        ServerPrompt::Password {
            endpoint,
            user_name,
            text_entry,
            ..
        } => widget(Cow::Owned(
            SECRET_DOT.repeat(text_entry.input.chars().count()),
        ))
        .answers(vec![
            answer("Link  ", endpoint.as_str(), active_theme),
            answer("User  ", user_name.as_str(), active_theme),
        ])
        .field_hint(PASSWORD_HINT)
        .error(text_entry.error.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        overlay::{ServerPrompt, TextEntry},
        server::{Endpoint, EndpointError, UserName},
    };

    use crate::{
        overlay::add_server::{LINK_HINT, prompt},
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

    fn endpoint() -> Endpoint {
        Endpoint::parse("https://music.example.com").unwrap()
    }

    fn frame(server_prompt: &ServerPrompt) -> String {
        frame_at(100, server_prompt)
    }

    fn frame_at(width: u16, server_prompt: &ServerPrompt) -> String {
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
        .to_string()
    }

    #[test]
    fn add_server_link_step_shows_the_typed_link() {
        insta::assert_snapshot!(frame(&ServerPrompt::Link {
            origin_server_name: None,
            text_entry: entry("https://music.example.com", None),
        }));
    }

    #[test]
    fn add_server_link_step_shows_the_error_under_the_field() {
        insta::assert_snapshot!(frame(&ServerPrompt::Link {
            origin_server_name: None,
            text_entry: entry("music.example.com", Some(EndpointError::Scheme)),
        }));
    }

    #[test]
    fn add_server_link_step_shows_the_live_verdict_under_the_hint() {
        insta::assert_snapshot!(frame(&ServerPrompt::Link {
            origin_server_name: None,
            text_entry: entry("https://", Some(EndpointError::Host)),
        }));
    }

    #[test]
    fn add_server_link_step_opens_with_its_whole_hint_at_80_columns() {
        let screen = frame_at(
            80,
            &ServerPrompt::Link {
                origin_server_name: None,
                text_entry: entry("", Some(EndpointError::Empty)),
            },
        );
        assert!(screen.contains(LINK_HINT));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn add_server_user_step_shows_the_link_above_the_field() {
        insta::assert_snapshot!(frame(&ServerPrompt::User {
            origin_server_name: None,
            endpoint: endpoint(),
            text_entry: entry("alice", None),
        }));
    }

    #[test]
    fn add_server_password_step_paints_a_dot_per_character() {
        let screen = frame(&ServerPrompt::Password {
            origin_server_name: None,
            endpoint: endpoint(),
            user_name: UserName::new("alice").unwrap(),
            text_entry: entry("hunter2", None),
        });
        assert!(!screen.contains("hunter2"));
        insta::assert_snapshot!(screen);
    }
}
