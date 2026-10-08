use std::borrow::Cow;

use kernel::domain::{geometry::Cells, overlay::TextEntry, time::TimecodeError};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(61);

pub(crate) fn prompt<'a>(
    text_entry: &'a TextEntry<TimecodeError>,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget::new(
        PromptBody::Entry(Cow::Borrowed(text_entry.input.as_str())),
        active_theme,
    )
    .title(glyphs::jump_to_time::TITLE_WORD)
    .hint(glyphs::jump_to_time::HINT)
    .min_width(MIN_WIDTH)
    .error(text_entry.error.as_ref())
}

#[cfg(test)]
mod tests {
    use kernel::domain::{overlay::TextEntry, time::TimecodeError};

    use crate::{
        overlay::jump_to_time::prompt,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn frame(text_entry: &TextEntry<TimecodeError>, width: u16, height: u16) -> String {
        let theme = noir();
        let prompt =
            prompt(text_entry, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered(width, height, |frame| {
            frame.render_widget(&prompt, frame.area());
        })
        .to_string()
    }

    #[test]
    fn jump_to_time_overlay_shows_title_input_and_hint() {
        let text_entry = TextEntry {
            input: "1:05".to_string(),
            error: None,
        };
        insta::assert_snapshot!(frame(&text_entry, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_shows_error_line_for_malformed_input() {
        let text_entry = TextEntry {
            input: "abc".to_string(),
            error: Some(TimecodeError::Malformed),
        };
        insta::assert_snapshot!(frame(&text_entry, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_does_not_panic_on_a_tiny_terminal() {
        assert_eq!(frame(&TextEntry::default(), 4, 3).lines().count(), 3);
    }
}
