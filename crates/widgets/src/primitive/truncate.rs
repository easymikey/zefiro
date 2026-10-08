use std::borrow::Cow;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::primitive::glyphs::ELLIPSIS;

fn cell_width(ch: char) -> usize {
    ch.width().unwrap_or(1)
}

#[must_use]
pub(crate) fn truncate(text: &str, width: usize) -> Cow<'_, str> {
    if width == 0 {
        return Cow::Borrowed("");
    }
    if text.width() <= width {
        return Cow::Borrowed(text);
    }
    let keep_width = width.saturating_sub(cell_width(ELLIPSIS));
    Cow::Owned(format!("{}{ELLIPSIS}", prefix(text, keep_width)))
}

fn prefix(text: &str, width: usize) -> &str {
    let end = text
        .char_indices()
        .scan(0usize, |kept_width, (offset, ch)| {
            *kept_width += cell_width(ch);
            (*kept_width <= width).then_some(offset + ch.len_utf8())
        })
        .last()
        .unwrap_or(0);
    text.get(..end).unwrap_or("")
}

#[must_use]
pub(crate) fn truncate_owned(text: String, width: usize) -> String {
    if width > 0 && text.width() <= width {
        return text;
    }
    truncate(&text, width).into_owned()
}

const BLANKS: &str = "                                                                                                                                                                                                                                                                ";

#[must_use]
pub(crate) fn blanks(width: usize) -> Cow<'static, str> {
    BLANKS
        .get(..width)
        .map_or_else(|| Cow::Owned(" ".repeat(width)), Cow::Borrowed)
}

#[must_use]
pub(crate) fn truncate_head(text: &str, width: usize) -> Cow<'_, str> {
    if width == 0 {
        return Cow::Borrowed("");
    }
    if text.width() <= width {
        return Cow::Borrowed(text);
    }
    let keep_width = width.saturating_sub(cell_width(ELLIPSIS));
    let start = text
        .char_indices()
        .rev()
        .scan(0usize, |kept_width, (offset, ch)| {
            *kept_width += cell_width(ch);
            (*kept_width <= keep_width).then_some(offset)
        })
        .last()
        .unwrap_or(text.len());
    Cow::Owned(format!("{ELLIPSIS}{}", text.get(start..).unwrap_or("")))
}

#[must_use]
pub(crate) fn truncate_line<'a>(line: Line<'a>, width: usize) -> Line<'a> {
    if line.width() <= width {
        return line;
    }
    let Some(keep_width) = width.checked_sub(cell_width(ELLIPSIS)) else {
        return Line::default();
    };
    let style = line
        .spans
        .first()
        .map_or_else(Style::default, |span| span.style);
    let spans: Vec<Span<'a>> = line
        .spans
        .into_iter()
        .scan(Some(keep_width), |budget, span| {
            let remaining = (*budget)?;
            *budget = remaining.checked_sub(span.content.width());
            if budget.is_some() {
                return Some(Some(span));
            }
            let kept = prefix(&span.content, remaining);
            Some((!kept.is_empty()).then(|| {
                crate::primitive::span::text(kept.to_owned())
                    .style(span.style)
                    .into()
            }))
        })
        .flatten()
        .collect();
    let ellipsis: Span<'a> = crate::primitive::span::text(String::from(ELLIPSIS))
        .style(spans.last().map_or(style, |span| span.style))
        .into();
    Line::from_iter(spans.into_iter().chain(std::iter::once(ellipsis)))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::{prop_assert, prop_assert_eq, proptest};
    use ratatui::{
        style::Color,
        text::{Line, Span},
    };
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::primitive::{
        glyphs::ELLIPSIS,
        span::{line, text},
        truncate::{truncate, truncate_line, truncate_owned},
    };

    proptest! {
        #[test]
        fn truncation_never_exceeds_requested_width(text in "(?s:.)*", width in 0usize..40) {
            let out = truncate(&text, width);
            prop_assert!(out.width() <= width);
        }

        #[test]
        fn truncation_never_splits_a_character(text in "(?s:.)*", width in 0usize..40) {
            let out = truncate(&text, width);
            if width == 0 {
                prop_assert_eq!(out.as_ref(), "");
            } else if text.width() <= width {
                prop_assert_eq!(out.as_ref(), text.as_str());
            } else {
                let kept = out.strip_suffix(ELLIPSIS);
                prop_assert!(
                    kept.is_some_and(|kept| text.starts_with(kept)),
                    "truncated output must end with the ellipsis and keep a prefix"
                );
            }
        }

        #[test]
        fn truncate_owned_matches_truncate(text in "(?s:.)*", width in 0usize..40) {
            let expected = truncate(&text, width).into_owned();
            prop_assert_eq!(truncate_owned(text, width), expected);
        }
    }

    #[rstest]
    #[case::fits("hello", 5)]
    #[case::cut("hello world", 6)]
    #[case::no_budget("hello", 0)]
    fn truncate_owned_keeps_a_fitting_text_and_cuts_like_truncate(
        #[case] text: &str,
        #[case] width: usize,
    ) {
        assert_eq!(
            truncate_owned(text.to_owned(), width),
            truncate(text, width)
        );
    }

    #[rstest]
    #[case::wide_glyphs("界🙂abc", 5, "界🙂…")]
    #[case::already_short("hi", 5, "hi")]
    #[case::no_budget("hello", 0, "")]
    #[case::a_control_character_costs_one_cell("\u{0}abcdef", 3, "\u{0}a…")]
    fn truncate_counts_display_cells(
        #[case] text: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let truncated = truncate(text, width);
        assert_eq!(truncated, expected);
        assert!(truncated.width() <= width);
        assert_eq!(
            truncate(text, width),
            truncated,
            "`truncate` is the same call"
        );
    }

    fn styled_line() -> Line<'static> {
        line([
            text("[Shuffle: on] ").fg(Color::White),
            text("[Repeat: Off] ").fg(Color::White),
            text("[Queue: 3] ").fg(Color::White),
            text("[2/9]").fg(Color::Yellow),
        ])
    }

    #[test]
    fn a_line_is_truncated_span_by_span() {
        let line = styled_line();
        let whole = line.width();

        let untouched = truncate_line(line.clone(), whole);
        assert_eq!(untouched.spans.len(), line.spans.len());
        assert_eq!(untouched.width(), whole);

        let cut = truncate_line(line, 20);
        assert!(cut.width() <= 20);
        let text: String = cut.spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(text.starts_with("[Shuffle: on] "));
        assert!(text.ends_with('…'));
        assert!(
            !text.contains("Queue"),
            "spans past the truncation point must be dropped, got:\n{text}"
        );

        let empty = truncate_line(styled_line(), 0);
        assert_eq!(empty.width(), 0);
    }

    #[test]
    fn a_line_cut_exactly_at_a_span_boundary_ends_with_the_ellipsis() {
        let line = styled_line();
        let first_width = line.spans.first().map_or(0, Span::width);
        assert_eq!(first_width, 14);

        let cut = truncate_line(line, first_width);
        assert!(cut.width() <= first_width);
        let text: String = cut.spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "[Shuffle: on]…");
        assert_eq!(
            cut.spans.last().map(|span| span.style.fg),
            Some(Some(Color::White)),
            "the ellipsis is styled like the last kept span"
        );
    }
}
