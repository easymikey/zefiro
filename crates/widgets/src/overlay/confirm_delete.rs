use kernel::domain::DeleteCandidate;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    overlay::modal::{OverlayAreas, Prompt, PromptBody},
    primitive::{canvas::Canvas, glyphs::ConfirmDeleteGlyphs},
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 24;

#[must_use]
fn sentence(candidate: &DeleteCandidate) -> String {
    let glyphs = ConfirmDeleteGlyphs::default();
    format!(
        "{}{}{}{}{}",
        glyphs.quote_open,
        candidate.title,
        glyphs.quote_close,
        glyphs.artist_separator,
        candidate.artist
    )
}

#[derive(Debug)]
pub(crate) struct ConfirmDeleteOverlay<'a> {
    pub(crate) candidate: &'a DeleteCandidate,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) avoid: &'a [Rect],
}

impl ConfirmDeleteOverlay<'_> {
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
        let glyphs = ConfirmDeleteGlyphs::default();
        Prompt {
            title: glyphs.title_word,
            hint: glyphs.hint,
            min_width: MIN_WIDTH,
            body: PromptBody::Sentence(sentence(self.candidate)),
            error: None,
            avoid: self.avoid,
            theme: self.theme,
        }
    }
}

impl Widget for &ConfirmDeleteOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{DeleteCandidate, PlaylistIndex};

    use crate::{
        overlay::confirm_delete::{ConfirmDeleteOverlay, sentence},
        scene::fixtures::{noir, painted},
        theme::{ActiveTheme, ColorDepth},
    };

    fn candidate() -> DeleteCandidate {
        DeleteCandidate {
            track: PlaylistIndex::new(0),
            title: "Moon River".to_string(),
            artist: "Audrey Hepburn".to_string(),
        }
    }

    #[test]
    fn the_sentence_quotes_the_title_and_names_the_artist() {
        let candidate = candidate();
        assert_eq!(sentence(&candidate), "\"Moon River\" — Audrey Hepburn");
    }

    #[test]
    fn confirm_delete_shows_the_quoted_title_and_artist() {
        let theme = noir();
        let candidate = candidate();
        let overlay = ConfirmDeleteOverlay {
            candidate: &candidate,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        insta::assert_snapshot!(painted(&overlay, 60, 12));
    }

    #[test]
    fn confirm_delete_does_not_panic_on_a_tiny_terminal() {
        let theme = noir();
        let candidate = candidate();
        let overlay = ConfirmDeleteOverlay {
            candidate: &candidate,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            avoid: &[],
        };
        let _ = painted(&overlay, 4, 3);
    }
}
