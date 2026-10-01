use std::borrow::Cow;

use ratatui::text::{Line, Span};
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
    let out: String = text
        .chars()
        .scan(0usize, |kept_width, ch| {
            *kept_width += cell_width(ch);
            (*kept_width <= keep_width).then_some(ch)
        })
        .chain(std::iter::once(ELLIPSIS))
        .collect();
    Cow::Owned(out)
}

const BLANKS: &str = "                                                                                                                                                                                                                                                                ";

#[must_use]
pub(crate) fn blanks(width: usize) -> Cow<'static, str> {
    BLANKS
        .get(..width)
        .map_or_else(|| Cow::Owned(" ".repeat(width)), Cow::Borrowed)
}

#[must_use]
pub(crate) fn truncate_from_left(text: &str, width: usize) -> Cow<'_, str> {
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
    let mut out = String::with_capacity(ELLIPSIS.len_utf8() + text.len() - start);
    out.push(ELLIPSIS);
    out.push_str(text.get(start..).unwrap_or_default());
    Cow::Owned(out)
}

#[must_use]
pub(crate) fn truncate_line_to_width<'a>(line: Line<'a>, width: usize) -> Line<'a> {
    if line.width() <= width {
        return line;
    }
    let clipped = |span: Span<'a>, budget: usize| {
        let clipped_text = truncate(&span.content, budget).into_owned();
        (!clipped_text.is_empty()).then(|| {
            crate::primitive::span::text(clipped_text)
                .style(span.style)
                .into()
        })
    };
    let spans: Vec<Span<'a>> = line
        .spans
        .into_iter()
        .scan(Some(width), |budget, span| {
            let remaining = (*budget)?;
            *budget = remaining.checked_sub(span.content.width());
            if budget.is_some() {
                return Some(Some(span));
            }
            Some(clipped(span, remaining))
        })
        .flatten()
        .collect();
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::{prop_assert, prop_assert_eq, proptest};
    use ratatui::{style::Color, text::Line};
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::primitive::{
        glyphs::ELLIPSIS,
        span::{line, text},
        text::{truncate, truncate_line_to_width},
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

        let untouched = truncate_line_to_width(line.clone(), whole);
        assert_eq!(untouched.spans.len(), line.spans.len());
        assert_eq!(untouched.width(), whole);

        let cut = truncate_line_to_width(line, 20);
        assert!(cut.width() <= 20);
        let text: String = cut.spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(text.starts_with("[Shuffle: on] "));
        assert!(text.ends_with('…'));
        assert!(
            !text.contains("Queue"),
            "spans past the truncation point must be dropped, got:\n{text}"
        );

        let empty = truncate_line_to_width(styled_line(), 0);
        assert_eq!(empty.width(), 0);
    }
}
