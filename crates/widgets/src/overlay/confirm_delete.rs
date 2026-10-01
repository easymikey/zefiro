use kernel::domain::DeleteCandidate;

use crate::{
    overlay::modal::{Prompt, PromptBody},
    primitive::glyphs,
    theme::ActiveTheme,
};

const MIN_WIDTH: u16 = 24;

#[must_use]
fn sentence(candidate: &DeleteCandidate) -> String {
    format!(
        "{}{}{}{}{}",
        glyphs::confirm_delete::QUOTE_OPEN,
        candidate.title,
        glyphs::confirm_delete::QUOTE_CLOSE,
        glyphs::confirm_delete::ARTIST_SEPARATOR,
        candidate.artist
    )
}

pub(crate) fn prompt<'a>(
    candidate: &DeleteCandidate,
    theme: ActiveTheme<'a>,
) -> Prompt<'a> {
    Prompt {
        title: glyphs::confirm_delete::TITLE_WORD,
        hint: glyphs::confirm_delete::HINT,
        min_width: MIN_WIDTH,
        body: PromptBody::Sentence(sentence(candidate)),
        error: None,
        avoid: &[],
        theme,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{DeleteCandidate, PlaylistIndex};

    use crate::{
        overlay::{
            confirm_delete::{prompt, sentence},
            rendered_canvas,
        },
        test_support::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn candidate() -> DeleteCandidate {
        DeleteCandidate {
            track: PlaylistIndex::new(0),
            title: "Moon River".to_string(),
            artist: "Audrey Hepburn".to_string(),
        }
    }

    fn frame(width: u16, height: u16) -> String {
        let theme = noir();
        let prompt = prompt(
            &candidate(),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        rendered_canvas(width, height, |canvas| {
            prompt.render_in(prompt.areas(canvas.area), canvas);
        })
        .to_string()
    }

    #[test]
    fn the_sentence_quotes_the_title_and_names_the_artist() {
        assert_eq!(sentence(&candidate()), "\"Moon River\" — Audrey Hepburn");
    }

    #[test]
    fn confirm_delete_shows_the_quoted_title_and_artist() {
        insta::assert_snapshot!(frame(60, 12));
    }

    #[test]
    fn confirm_delete_does_not_panic_on_a_tiny_terminal() {
        let _ = frame(4, 3);
    }
}
