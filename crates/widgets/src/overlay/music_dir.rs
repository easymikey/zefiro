use std::borrow::Cow;

use kernel::domain::{
    geometry::Cells,
    overlay::{MusicDirError, TextEntry},
};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(40);

pub(crate) fn prompt<'a>(
    text_entry: &'a TextEntry<MusicDirError>,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget::new(
        PromptBody::Entry(Cow::Borrowed(text_entry.input.as_str())),
        active_theme,
    )
    .title(glyphs::music_dir::TITLE_WORD)
    .hint(glyphs::music_dir::HINT)
    .min_width(MIN_WIDTH)
    .error(text_entry.error.as_ref())
}

#[cfg(test)]
mod tests {
    use kernel::domain::overlay::{MusicDirError, TextEntry};

    use crate::{
        overlay::music_dir::prompt,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn frame(input: &str, error: Option<MusicDirError>, size: (u16, u16)) -> String {
        let theme = noir();
        let text_entry = TextEntry {
            input: input.to_string(),
            error,
        };
        let prompt =
            prompt(&text_entry, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered(size.0, size.1, |frame| {
            frame.render_widget(&prompt, frame.area());
        })
        .to_string()
    }

    #[test]
    fn music_dir_overlay_shows_title_input_and_hint() {
        insta::assert_snapshot!(frame("/home/user/Music", None, (80, 24)));
    }

    #[test]
    fn music_dir_overlay_shows_the_error_line_when_the_folder_is_empty() {
        insta::assert_snapshot!(frame("", Some(MusicDirError::Empty), (80, 24)));
    }

    #[test]
    fn a_long_path_keeps_its_tail_and_cursor_in_view() {
        let screen = frame(&("/a".repeat(60) + "/end"), None, (80, 24));
        assert!(
            screen.lines().any(|row| row.contains("end_")),
            "the input row must show the typed tail and the cursor:\n{screen}"
        );
    }

    #[test]
    fn music_dir_overlay_does_not_panic_on_a_tiny_terminal() {
        assert_eq!(frame("", None, (4, 3)).lines().count(), 3);
    }
}
