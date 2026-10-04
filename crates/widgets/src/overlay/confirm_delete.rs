use kernel::domain::{DeleteCandidate, geometry::Cells};

use crate::{
    overlay::modal::{PromptBody, PromptStyle, PromptWidget},
    primitive::glyphs,
    theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(24);

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
) -> PromptWidget<'a> {
    PromptWidget {
        title: glyphs::confirm_delete::TITLE_WORD,
        hint: glyphs::confirm_delete::HINT,
        min_width: MIN_WIDTH,
        body: PromptBody::Sentence(sentence(candidate)),
        error: None,
        avoid: &[],
        style: PromptStyle::from_theme(&theme),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::DeleteCandidate;

    use crate::{
        overlay::confirm_delete::{prompt, sentence},
        test_support::{noir, rendered},
        theme::{ActiveTheme, ColorDepth},
    };

    fn candidate() -> DeleteCandidate {
        DeleteCandidate {
            source: kernel::TrackRef::Local("/music/moon.flac".into()),
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
        rendered(width, height, |frame| {
            frame.render_widget(&prompt, frame.area());
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
        assert_eq!(frame(4, 3).lines().count(), 3);
    }
}
