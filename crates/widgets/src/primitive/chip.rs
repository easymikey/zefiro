use kernel::domain::{Speed, appearance::SpeedChip};
use ratatui::{style::Color, text::Span};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{glyphs, span::text},
    theme::{ActiveTheme, Role},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChipStyle {
    pub border: Color,
    pub foreground: Color,
}

impl ChipStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            border: theme.role(Role::Dim),
            foreground: theme.role(Role::Text),
        }
    }
}

#[must_use]
pub(crate) fn spans(label: &str, colors: ChipStyle) -> Vec<Span<'static>> {
    vec![
        text(format!("{}{}", glyphs::chip::OPEN, glyphs::chip::PAD))
            .fg(colors.border)
            .into(),
        text(label.to_uppercase()).fg(colors.foreground).into(),
        text(format!("{}{}", glyphs::chip::PAD, glyphs::chip::CLOSE))
            .fg(colors.border)
            .into(),
    ]
}

#[must_use]
pub(crate) fn compact(label: &str) -> String {
    format!(
        "{}{}{}",
        glyphs::chip::OPEN,
        label.to_lowercase(),
        glyphs::chip::CLOSE
    )
}

#[must_use]
pub(crate) fn width(label: &str) -> u16 {
    let decoration = format!(
        "{}{}{}{}",
        glyphs::chip::OPEN,
        glyphs::chip::PAD,
        glyphs::chip::PAD,
        glyphs::chip::CLOSE
    );
    let cells = decoration.width() + label.to_uppercase().width();
    u16::try_from(cells).unwrap_or(u16::MAX)
}

fn speed_chip_text(speed: Speed, mode: SpeedChip) -> Option<String> {
    let label = match mode {
        SpeedChip::Never => None,
        SpeedChip::Changed => speed.label(),
        SpeedChip::Always => Some(speed.to_string()),
    };
    label.map(|label| format!("{label}{}", glyphs::speed_chip::MULTIPLY))
}

#[must_use]
pub(crate) fn speed_chip_width(speed: Speed, mode: SpeedChip) -> u16 {
    let Some(label) = speed_chip_text(speed, mode) else {
        return 0;
    };
    let width = glyphs::speed_chip::GAP.width()
        + glyphs::speed_chip::MARKER.width()
        + label.width();
    u16::try_from(width).unwrap_or(u16::MAX)
}

#[must_use]
pub(crate) fn speed_chip_spans(
    speed: Speed,
    mode: SpeedChip,
    colors: ChipStyle,
) -> Vec<Span<'static>> {
    let Some(label) = speed_chip_text(speed, mode) else {
        return Vec::new();
    };
    vec![
        text(glyphs::speed_chip::GAP).into(),
        text(glyphs::speed_chip::MARKER)
            .fg(colors.border)
            .dim()
            .into(),
        text(label).fg(colors.foreground).dim().into(),
    ]
}

#[cfg(test)]
mod tests {
    use kernel::{
        Bounded,
        domain::{Speed, appearance::SpeedChip},
    };
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::primitive::chip::{
        ChipStyle,
        spans,
        speed_chip_spans,
        speed_chip_width,
    };

    fn colors() -> ChipStyle {
        ChipStyle {
            border: Color::DarkGray,
            foreground: Color::Cyan,
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
        #[case] mode: SpeedChip,
        #[case] expected: Option<&str>,
    ) {
        let spans = speed_chip_spans(speed, mode, colors());
        let text = (!spans.is_empty()).then(|| {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });
        assert_eq!(text.as_deref(), expected);

        let rendered: usize = spans.iter().map(|span| span.content.width()).sum();
        assert_eq!(
            usize::from(speed_chip_width(speed, mode)),
            rendered,
            "the reserved width must match what is painted"
        );
    }

    #[test]
    fn the_marker_and_the_value_carry_the_dim_face_and_their_own_colours() {
        let spans =
            speed_chip_spans(Speed::clamped(1.25), SpeedChip::Changed, colors());
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
        let colors = ChipStyle {
            border: Color::White,
            foreground: Color::Red,
        };
        spans(text, colors)
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
