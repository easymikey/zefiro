use kernel::domain::{geometry::Cells, overlay::JumpDigits};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptStyle, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(61);

pub(crate) fn prompt<'a>(
    digits: &'a JumpDigits,
    theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget {
        title: glyphs::jump_to_time::TITLE_WORD,
        hint: glyphs::jump_to_time::HINT,
        min_width: MIN_WIDTH,
        body: PromptBody::Entry(&digits.input),
        error: digits.error.as_ref().map(ToString::to_string),
        avoid: &[],
        style: PromptStyle::from_theme(&theme),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{overlay::JumpDigits, time::TimecodeError};

    use crate::{
        overlay::jump_to_time::prompt,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn frame(digits: &JumpDigits, width: u16, height: u16) -> String {
        let theme = noir();
        let prompt = prompt(digits, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered(width, height, |frame| {
            frame.render_widget(&prompt, frame.area());
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
        assert_eq!(frame(&JumpDigits::default(), 4, 3).lines().count(), 3);
    }
}
