use std::borrow::Cow;

use kernel::domain::{appearance::SpeedChip, geometry::Cells, speed::Speed};
use ratatui::{style::Color, text::Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    pixels::numeric::small_count_u16,
    primitive::{glyphs, span::text},
    theme::colors::Colors,
};

#[must_use]
pub(crate) fn spans<'a>(
    label: impl Into<Cow<'a, str>>,
    colors: &Colors<Color>,
) -> Vec<Span<'a>> {
    let label = label.into();
    let label = if label.chars().any(char::is_lowercase) {
        Cow::Owned(label.to_uppercase())
    } else {
        label
    };
    vec![
        text(glyphs::chip::OPEN_PAD)
            .fg(colors.muted_foreground)
            .into(),
        text(label).fg(colors.foreground).into(),
        text(glyphs::chip::PAD_CLOSE)
            .fg(colors.muted_foreground)
            .into(),
    ]
}

#[must_use]
pub(crate) fn width(label: &str) -> Cells {
    let cells = label
        .chars()
        .flat_map(char::to_uppercase)
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>();
    Cells(small_count_u16(
        glyphs::chip::OPEN_PAD.width() + glyphs::chip::PAD_CLOSE.width() + cells,
    ))
}

fn speed_chip_text(speed: Speed, speed_chip: SpeedChip) -> Option<String> {
    let label = match speed_chip {
        SpeedChip::Never => None,
        SpeedChip::Changed => (speed != Speed::default()).then(|| speed.to_string()),
        SpeedChip::Always => Some(speed.to_string()),
    };
    label.map(|label| format!("{label}{}", glyphs::speed_chip::MULTIPLY))
}

#[must_use]
pub(crate) fn speed_chip_width(speed: Speed, speed_chip: SpeedChip) -> Cells {
    let Some(label) = speed_chip_text(speed, speed_chip) else {
        return Cells(0);
    };
    let width = glyphs::speed_chip::GAP.width()
        + glyphs::speed_chip::MARKER.width()
        + label.width();
    Cells(small_count_u16(width))
}

#[must_use]
pub(crate) fn speed_chip_spans(
    speed: Speed,
    speed_chip: SpeedChip,
    colors: &Colors<Color>,
) -> Vec<Span<'static>> {
    let Some(label) = speed_chip_text(speed, speed_chip) else {
        return Vec::new();
    };
    vec![
        text(glyphs::speed_chip::GAP).into(),
        text(glyphs::speed_chip::MARKER)
            .fg(colors.muted_foreground)
            .dim()
            .into(),
        text(label).fg(colors.accent).dim().into(),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::domain::{appearance::SpeedChip, bounded::Bounded, speed::Speed};
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::{
        primitive::chip::{spans, speed_chip_spans, speed_chip_width},
        theme::colors::Colors,
    };

    fn colors() -> Colors<Color> {
        Colors {
            muted_foreground: Color::DarkGray,
            accent: Color::Cyan,
            ..Colors::default()
        }
    }

    #[rstest]
    #[case::changed_at_the_stock_speed(Speed::default(), SpeedChip::Changed, None)]
    #[case::changed_after_a_change(
        Speed::clamped(1.25),
        SpeedChip::Changed,
        Some("  \u{00BB} 1.25\u{00D7}")
    )]
    #[case::always_at_the_stock_speed(
        Speed::default(),
        SpeedChip::Always,
        Some("  \u{00BB} 1\u{00D7}")
    )]
    #[case::never_at_the_stock_speed(Speed::default(), SpeedChip::Never, None)]
    #[case::never_after_a_change(Speed::clamped(1.25), SpeedChip::Never, None)]
    fn the_speed_chip_reads_its_mode(
        #[case] speed: Speed,
        #[case] speed_chip: SpeedChip,
        #[case] expected: Option<&str>,
    ) {
        let spans = speed_chip_spans(speed, speed_chip, &colors());
        let text = (!spans.is_empty()).then(|| {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });
        assert_eq!(text.as_deref(), expected);

        let rendered: usize = spans.iter().map(|span| span.content.width()).sum();
        assert_eq!(
            speed_chip_width(speed, speed_chip).count(),
            rendered,
            "the reserved width must match what is painted"
        );
    }

    #[test]
    fn the_marker_and_the_value_carry_the_dim_face_and_their_own_colours() {
        let spans =
            speed_chip_spans(Speed::clamped(1.25), SpeedChip::Changed, &colors());
        let faces: Vec<(Option<Color>, bool)> = spans
            .iter()
            .skip(1)
            .take(2)
            .map(|span| {
                (
                    span.style.fg,
                    span.style.add_modifier.contains(Modifier::DIM),
                )
            })
            .collect();
        assert_eq!(
            faces,
            vec![(Some(Color::DarkGray), true), (Some(Color::Cyan), true)]
        );
    }

    fn joined_generic(text: &str) -> String {
        let colors = Colors {
            muted_foreground: Color::White,
            foreground: Color::Red,
            ..Colors::default()
        };
        spans(text, &colors)
            .iter()
            .map(|span| span.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::lowercase("320 kbps", "[ 320 KBPS ]")]
    #[case::already_uppercase("MP3", "[ MP3 ]")]
    fn a_generic_chip_wraps_and_uppercases_its_value(
        #[case] text: &str,
        #[case] painted: &str,
    ) {
        assert_eq!(joined_generic(text), painted);
    }
}
