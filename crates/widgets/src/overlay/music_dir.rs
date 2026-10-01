use kernel::domain::{MusicDirError, TextEntry};

use crate::{
    overlay::modal::{Prompt, PromptBody},
    primitive::glyphs,
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 40;

pub(crate) fn prompt<'a>(
    typed: &'a TextEntry,
    error: Option<&MusicDirError>,
    theme: ActiveTheme<'a>,
) -> Prompt<'a> {
    Prompt {
        title: glyphs::music_dir::TITLE_WORD,
        hint: glyphs::music_dir::HINT,
        min_width: MIN_WIDTH,
        body: PromptBody::Entry(&typed.input),
        error: error.map(ToString::to_string),
        avoid: &[],
        theme,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{MusicDirError, TextEntry};

    use crate::{
        overlay::{music_dir::prompt, rendered_canvas},
        test_support::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn frame(input: &str, error: Option<MusicDirError>, size: (u16, u16)) -> String {
        let theme = noir();
        let typed = TextEntry {
            input: input.to_string(),
        };
        let prompt = prompt(
            &typed,
            error.as_ref(),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        rendered_canvas(size.0, size.1, |canvas| {
            prompt.render_in(prompt.areas(canvas.area), canvas);
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
    fn music_dir_overlay_does_not_panic_on_a_tiny_terminal() {
        let _ = frame("", None, (4, 3));
    }
}
