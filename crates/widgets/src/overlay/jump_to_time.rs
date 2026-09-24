use kernel::domain::JumpDigits;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    overlay::modal::{OverlayAreas, Prompt, PromptBody},
    primitive::{canvas::Canvas, glyphs::JumpToTimeGlyphs},
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 61;

#[derive(Debug)]
pub(crate) struct JumpToTimeOverlay<'a> {
    pub(crate) digits: &'a JumpDigits,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) avoid: &'a [Rect],
}

impl JumpToTimeOverlay<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(self.prompt().areas(screen))
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        if let OverlayAreas::Dialog(areas) = areas {
            self.prompt().render_in(areas, canvas);
        }
    }

    fn prompt(&self) -> Prompt<'_> {
        let glyphs = JumpToTimeGlyphs::default();
        Prompt {
            title: glyphs.title_word,
            hint: glyphs.hint,
            min_width: MIN_WIDTH,
            body: PromptBody::Entry(&self.digits.input),
            error: self.digits.error.as_ref().map(ToString::to_string),
            avoid: self.avoid,
            theme: self.theme,
        }
    }
}

impl Widget for &JumpToTimeOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{JumpDigits, TimecodeError};

    use crate::{
        overlay::jump_to_time::JumpToTimeOverlay,
        scene::fixtures::{noir, painted},
        theme::{ActiveTheme, ColorDepth},
    };

    #[test]
    fn jump_to_time_overlay_shows_title_input_and_hint() {
        let theme = noir();
        let digits = JumpDigits {
            input: "1:05".to_string(),
            error: None,
        };
        let overlay = JumpToTimeOverlay {
            digits: &digits,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_shows_error_line_for_malformed_input() {
        let theme = noir();
        let digits = JumpDigits {
            input: "abc".to_string(),
            error: Some(TimecodeError::Malformed),
        };
        let overlay = JumpToTimeOverlay {
            digits: &digits,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 24));
    }

    #[test]
    fn jump_to_time_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let digits = JumpDigits {
            input: String::new(),
            error: None,
        };
        let overlay = JumpToTimeOverlay {
            digits: &digits,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        let _ = painted(&overlay, 4, 3);
    }
}
