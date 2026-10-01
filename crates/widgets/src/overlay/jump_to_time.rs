use kernel::domain::JumpDigits;

use crate::{
    overlay::modal::{Prompt, PromptBody},
    primitive::glyphs,
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 61;

pub(crate) fn prompt<'a>(digits: &'a JumpDigits, theme: ActiveTheme<'a>) -> Prompt<'a> {
    Prompt {
        title: glyphs::jump_to_time::TITLE_WORD,
        hint: glyphs::jump_to_time::HINT,
        min_width: MIN_WIDTH,
        body: PromptBody::Entry(&digits.input),
        error: digits.error.as_ref().map(ToString::to_string),
        avoid: &[],
        theme,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{JumpDigits, TimecodeError};

    use crate::{
        overlay::{jump_to_time::prompt, rendered_canvas},
        test_support::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn frame(digits: &JumpDigits, width: u16, height: u16) -> String {
        let theme = noir();
        let prompt = prompt(digits, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered_canvas(width, height, |canvas| {
            prompt.render_in(prompt.areas(canvas.area), canvas);
        })
        .to_string()
    }

    #[test]
    fn jump_to_time_overlay_shows_title_input_and_hint() {
        let digits = JumpDigits {
            input: "1:05".to_string(),
            error: None,
        };
        insta::assert_snapshot!(frame(&digits, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_shows_error_line_for_malformed_input() {
        let digits = JumpDigits {
            input: "abc".to_string(),
            error: Some(TimecodeError::Malformed),
        };
        insta::assert_snapshot!(frame(&digits, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_does_not_panic_on_a_tiny_terminal() {
        let _ = frame(&JumpDigits::default(), 4, 3);
    }
}
