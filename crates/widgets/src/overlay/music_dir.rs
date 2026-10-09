use std::{borrow::Cow, iter};

use kernel::domain::{
    cursor_over::CursorOver,
    geometry::Cells,
    overlay::{Folders, MusicDirError, Subfolder, TextEntry, Verdict},
};
use ratatui::text::Line;

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::{
        glyphs,
        span::{line, text},
    },
    repaint::Presence,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(40);
const ROWS: usize = 8;

pub(crate) fn prompt<'a>(
    text_entry: &'a TextEntry<MusicDirError>,
    verdict: Option<Verdict>,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    let hint = match verdict {
        Some(Verdict::Readable) => glyphs::music_dir::readable::HINT,
        Some(Verdict::Denied) => glyphs::music_dir::denied::HINT,
        Some(Verdict::Missing | Verdict::NotADirectory | Verdict::Unreadable(_))
        | None => glyphs::music_dir::HINT,
    };
    PromptWidget::new(
        PromptBody::Entry(Cow::Borrowed(text_entry.input.as_str())),
        active_theme,
    )
    .title(glyphs::music_dir::TITLE_WORD)
    .hint(hint)
    .min_width(MIN_WIDTH)
    .verdict(verdict)
    .error(text_entry.error.as_ref())
    .spinner(Presence::from(
        text_entry.error == Some(MusicDirError::Pending),
    ))
}

pub(crate) fn rows<'a>(
    folders: &'a CursorOver<Folders>,
    active_theme: ActiveTheme<'a>,
) -> Vec<Line<'a>> {
    let CursorOver {
        cursor,
        content:
            Folders {
                path: _,
                subfolders,
                matches,
                revision: _,
            },
    } = folders;
    let colors = active_theme.colors();
    let entries = subfolders
        .as_ref()
        .map_or(&[][..], |listing| listing.subfolders.as_slice());
    matches
        .iter()
        .enumerate()
        .skip(cursor.index().saturating_sub(ROWS - 1))
        .take(ROWS)
        .filter_map(|(position, index)| {
            let subfolder = entries.get(*index)?;
            let (marker, color) = if position == cursor.index() {
                (glyphs::music_dir::SELECTED_MARKER, colors.foreground)
            } else {
                (
                    glyphs::music_dir::UNSELECTED_MARKER,
                    colors.muted_foreground,
                )
            };
            let glyph = match subfolder {
                Subfolder::Audio(_) => glyphs::music_dir::AUDIO,
                Subfolder::Plain(_) => glyphs::music_dir::PLAIN,
            };
            Some(line([
                text(marker).fg(color),
                text(glyph).fg(colors.highlight),
                text(subfolder.name()).fg(color),
            ]))
        })
        .chain(iter::repeat_with(Line::default))
        .take(ROWS)
        .collect()
}

pub(crate) fn listed(
    folders: &CursorOver<Folders>,
    verdict: Option<Verdict>,
) -> Option<Verdict> {
    let listed = folders
        .content
        .subfolders
        .as_ref()
        .filter(|_| folders.content.revision.is_none())?;
    match listed.verdict {
        Verdict::Readable => None,
        Verdict::Missing
        | Verdict::NotADirectory
        | Verdict::Denied
        | Verdict::Unreadable(_) => {
            (verdict != Some(listed.verdict)).then_some(listed.verdict)
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::overlay::{MusicDirError, TextEntry, Verdict};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{modal::frame::ModalAreas, music_dir::prompt},
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn entry(input: &str, error: Option<MusicDirError>) -> TextEntry<MusicDirError> {
        TextEntry {
            input: input.to_string(),
            error,
        }
    }

    fn areas(
        text_entry: &TextEntry<MusicDirError>,
        verdict: Option<Verdict>,
    ) -> ModalAreas {
        let theme = noir();
        prompt(
            text_entry,
            verdict,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .areas(Rect::new(0, 0, 80, 24), &[])
    }

    fn frame(
        text_entry: &TextEntry<MusicDirError>,
        verdict: Option<Verdict>,
        size: (u16, u16),
    ) -> String {
        let theme = noir();
        let prompt = prompt(
            text_entry,
            verdict,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        rendered(size.0, size.1, |frame| {
            frame.render_widget(&prompt, frame.area());
        })
        .to_string()
    }

    #[test]
    fn music_dir_overlay_shows_title_input_and_hint() {
        insta::assert_snapshot!(frame(
            &entry("/home/user/Music", None),
            None,
            (80, 24)
        ));
    }

    #[test]
    fn music_dir_overlay_shows_the_error_line_when_the_folder_is_empty() {
        insta::assert_snapshot!(frame(
            &entry("", Some(MusicDirError::Empty)),
            None,
            (80, 24)
        ));
    }

    #[test]
    fn music_dir_overlay_shows_a_denied_folder_and_the_settings_hint() {
        insta::assert_snapshot!(frame(
            &entry("/Users/me/Desktop", None),
            Some(Verdict::Denied),
            (100, 24)
        ));
    }

    #[test]
    fn music_dir_overlay_offers_save_on_a_readable_folder() {
        insta::assert_snapshot!(frame(
            &entry("/Users/me/Music", None),
            Some(Verdict::Readable),
            (80, 24)
        ));
    }

    #[test]
    fn music_dir_overlay_offers_check_on_a_missing_folder() {
        insta::assert_snapshot!(frame(
            &entry("/Users/me/Gone", None),
            Some(Verdict::Missing),
            (80, 24)
        ));
    }

    #[test]
    fn a_denied_verdict_is_cut_to_a_narrow_modal() {
        let screen = frame(
            &entry("/Users/me/Desktop", None),
            Some(Verdict::Denied),
            (50, 24),
        );
        assert!(
            screen
                .lines()
                .any(|row| row.contains("no permission") && row.contains('\u{2026}')),
            "the verdict row must end in an ellipsis inside the modal:\n{screen}"
        );
    }

    #[test]
    fn enter_before_the_answer_shows_the_folder_is_being_checked() {
        let screen = frame(
            &entry("/music", Some(MusicDirError::Pending)),
            None,
            (80, 24),
        );
        assert!(
            screen
                .lines()
                .any(|row| row.contains("⣾ checking the folder")),
            "the prompt must say the folder is being checked:\n{screen}"
        );
    }

    #[test]
    fn the_prompt_keeps_its_area_while_the_first_check_runs() {
        let theme = noir();
        let text_entry = entry("/Users/me/Music", None);
        let areas = |verdict| {
            prompt(
                &text_entry,
                verdict,
                ActiveTheme::new(&theme, ColorDepth::TrueColor),
            )
            .areas(Rect::new(0, 0, 80, 24), &[])
        };
        assert_eq!(areas(None), areas(Some(Verdict::Missing)));
    }

    #[rstest]
    #[case::enter_with_a_check_pending(
        entry("/music", Some(MusicDirError::Pending)),
        None
    )]
    #[case::an_answer(entry("/music", None), Some(Verdict::Missing))]
    #[case::enter_on_an_empty_path(entry("", Some(MusicDirError::Empty)), None)]
    fn the_prompt_keeps_the_area_it_opened_with(
        #[case] text_entry: TextEntry<MusicDirError>,
        #[case] verdict: Option<Verdict>,
    ) {
        assert_eq!(
            areas(&entry("/music", None), None),
            areas(&text_entry, verdict)
        );
    }

    #[test]
    fn a_long_path_keeps_its_tail_and_cursor_in_view() {
        let screen = frame(&entry(&("/a".repeat(60) + "/end"), None), None, (80, 24));
        assert!(
            screen.lines().any(|row| row.contains("/end ")),
            "the input row must show the typed tail and the cursor:\n{screen}"
        );
    }
}
