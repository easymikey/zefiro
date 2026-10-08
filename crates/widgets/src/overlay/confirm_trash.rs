use kernel::domain::{geometry::Cells, track::Track};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(24);

#[must_use]
fn sentence(track: &Track) -> [&str; 5] {
    [
        glyphs::quote::QUOTE_OPEN,
        track.title(),
        glyphs::quote::QUOTE_CLOSE,
        glyphs::confirm_trash::ARTIST_SEPARATOR,
        track.tags().artist.as_deref().unwrap_or(""),
    ]
}

pub(crate) fn prompt<'a>(
    track: &'a Track,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget::new(PromptBody::Sentence(sentence(track)), active_theme)
        .title(glyphs::confirm_trash::TITLE_WORD)
        .hint(glyphs::confirm_trash::HINT)
        .min_width(MIN_WIDTH)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::track::{AudioFormat, Tags, Track, TrackParts};

    use crate::{
        overlay::confirm_trash::prompt,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn track() -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: "/music/moon.flac".into(),
            duration: Duration::from_secs(201),
            tags: Tags {
                title: Some("Moon River".to_string()),
                artist: Some("Audrey Hepburn".to_string()),
                ..Tags::default()
            },
            audio_format: AudioFormat::default(),
        }))
    }

    fn frame(width: u16, height: u16) -> String {
        let theme = noir();
        let track = track();
        let prompt = prompt(&track, ActiveTheme::new(&theme, ColorDepth::TrueColor));
        rendered(width, height, |frame| {
            frame.render_widget(&prompt, frame.area());
        })
        .to_string()
    }

    #[test]
    fn confirm_trash_shows_the_quoted_title_and_artist() {
        insta::assert_snapshot!(frame(60, 12));
    }
}
