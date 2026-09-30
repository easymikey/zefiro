use kernel::domain::{MusicDirError, TextEntry};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    overlay::modal::{OverlayAreas, Prompt, PromptBody},
    primitive::{canvas::Canvas, glyphs::MusicDirGlyphs},
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 40;

#[derive(Debug)]
pub(crate) struct MusicDirOverlay<'a> {
    pub(crate) typed: &'a TextEntry,
    pub(crate) error: Option<&'a MusicDirError>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) avoid: &'a [Rect],
}

impl MusicDirOverlay<'_> {
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
        let glyphs = MusicDirGlyphs::default();
        Prompt {
            title: glyphs.title_word,
            hint: glyphs.hint,
            min_width: MIN_WIDTH,
            body: PromptBody::Entry(&self.typed.input),
            error: self.error.map(ToString::to_string),
            avoid: self.avoid,
            theme: self.theme,
        }
    }
}

impl Widget for &MusicDirOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{MusicDirError, TextEntry};

    use crate::{
        overlay::music_dir::MusicDirOverlay,
        scene::fixtures::{noir, painted},
        theme::{ActiveTheme, ColorDepth},
    };

    #[test]
    fn source_dir_overlay_shows_title_input_and_hint() {
        let theme = noir();
        let typed = TextEntry {
            input: "/home/user/Music".to_string(),
        };
        let overlay = MusicDirOverlay {
            typed: &typed,
            error: None,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 24));
    }

    #[test]
    fn source_dir_overlay_shows_the_error_line_when_the_folder_is_empty() {
        let theme = noir();
        let typed = TextEntry {
            input: String::new(),
        };
        let error = MusicDirError::Empty;
        let overlay = MusicDirOverlay {
            typed: &typed,
            error: Some(&error),
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 80, 24));
    }

    #[test]
    fn source_dir_overlay_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let typed = TextEntry {
            input: String::new(),
        };
        let overlay = MusicDirOverlay {
            typed: &typed,
            error: None,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        let _ = painted(&overlay, 4, 3);
    }
}
