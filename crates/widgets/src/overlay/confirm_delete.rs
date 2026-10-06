use kernel::domain::{geometry::Cells, overlay::DeleteCandidate};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(24);

#[must_use]
fn sentence(candidate: &DeleteCandidate) -> [&str; 5] {
    [
        glyphs::confirm_delete::QUOTE_OPEN,
        &candidate.title,
        glyphs::confirm_delete::QUOTE_CLOSE,
        glyphs::confirm_delete::ARTIST_SEPARATOR,
        &candidate.artist,
    ]
}

pub(crate) fn prompt<'a>(
    candidate: &'a DeleteCandidate,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget::new(PromptBody::Sentence(sentence(candidate)), active_theme)
        .title(glyphs::confirm_delete::TITLE_WORD)
        .hint(glyphs::confirm_delete::HINT)
        .min_width(MIN_WIDTH)
}

#[cfg(test)]
mod tests {
    use kernel::domain::overlay::DeleteCandidate;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    use crate::{
        overlay::confirm_delete::{prompt, sentence},
        primitive::canvas::Canvas,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn candidate() -> DeleteCandidate {
        DeleteCandidate {
            source: kernel::domain::track::TrackRef::Local("/music/moon.flac".into()),
            title: "Moon River".to_string(),
            artist: "Audrey Hepburn".to_string(),
        }
    }

    fn frame(width: u16, height: u16) -> String {
        let theme = noir();
        let candidate = candidate();
        let prompt =
            prompt(&candidate, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered(width, height, |frame| {
            frame.render_widget(&prompt, frame.area());
        })
        .to_string()
    }

    #[test]
    fn the_sentence_quotes_the_title_and_names_the_artist() {
        assert_eq!(
            sentence(&candidate()).concat(),
            "\"Moon River\" — Audrey Hepburn"
        );
    }

    #[test]
    fn confirm_delete_shows_the_quoted_title_and_artist() {
        insta::assert_snapshot!(frame(60, 12));
    }

    #[test]
    fn prompt_paints_the_areas_it_is_given() {
        let theme = noir();
        let candidate = candidate();
        let widget =
            prompt(&candidate, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        let rect = Rect::new(0, 0, 60, 12);
        let mut buffer = Buffer::empty(rect);
        widget.paint(
            widget.areas(rect),
            Canvas {
                area: rect,
                buffer: &mut buffer,
            },
        );
        let mut rendered = Buffer::empty(rect);
        (&widget).render(rect, &mut rendered);
        assert_ne!(buffer, Buffer::empty(rect));
        assert_eq!(buffer, rendered);
    }

    #[test]
    fn confirm_delete_does_not_panic_on_a_tiny_terminal() {
        assert_eq!(frame(4, 3).lines().count(), 3);
    }
}
