use config::SpeedChip;
use kernel::domain::Speed;
use ratatui::{style::Color, text::Span};
use unicode_width::UnicodeWidthStr;

use crate::primitive::{
    glyphs::{ChipGlyphs, SpeedChipGlyphs},
    span::text,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChipColors {
    pub border: Color,
    pub value: Color,
}

#[must_use]
pub(crate) fn spans(label: &str, colors: ChipColors) -> Vec<Span<'static>> {
    let glyphs = ChipGlyphs::default();
    vec![
        text(format!("{}{}", glyphs.open, glyphs.pad))
            .fg(colors.border)
            .into(),
        text(label.to_uppercase()).fg(colors.value).into(),
        text(format!("{}{}", glyphs.pad, glyphs.close))
            .fg(colors.border)
            .into(),
    ]
}

#[must_use]
pub(crate) fn compact(label: &str) -> String {
    let glyphs = ChipGlyphs::default();
    format!("{}{}{}", glyphs.open, label.to_lowercase(), glyphs.close)
}

#[must_use]
pub(crate) fn width(label: &str) -> u16 {
    let glyphs = ChipGlyphs::default();
    let decoration = format!(
        "{}{}{}{}",
        glyphs.open, glyphs.pad, glyphs.pad, glyphs.close
    );
    let cells = decoration.width() + label.to_uppercase().width();
    u16::try_from(cells).unwrap_or(u16::MAX)
}

fn speed_chip_text(speed: Speed, mode: SpeedChip) -> Option<String> {
    let glyphs = SpeedChipGlyphs::default();
    let label = match mode {
        SpeedChip::Never => None,
        SpeedChip::Changed => speed.label(),
        SpeedChip::Always => Some(speed.label_always()),
    };
    label.map(|label| format!("{label}{}", glyphs.multiply))
}

#[must_use]
pub(crate) fn speed_chip_width(speed: Speed, mode: SpeedChip) -> u16 {
    let Some(label) = speed_chip_text(speed, mode) else {
        return 0;
    };
    let glyphs = SpeedChipGlyphs::default();
    let width = glyphs.gap.width() + glyphs.marker.width() + label.width();
    u16::try_from(width).unwrap_or(u16::MAX)
}

#[must_use]
pub(crate) fn speed_chip_spans(
    speed: Speed,
    mode: SpeedChip,
    colors: ChipColors,
) -> Option<Vec<Span<'static>>> {
    let label = speed_chip_text(speed, mode)?;
    let glyphs = SpeedChipGlyphs::default();
    Some(vec![
        text(glyphs.gap).into(),
        text(glyphs.marker).fg(colors.border).dim().into(),
        text(label).fg(colors.value).dim().into(),
    ])
}

#[cfg(test)]
mod tests {
    use config::SpeedChip;
    use kernel::{Bounded, domain::Speed};
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;
    use unicode_width::UnicodeWidthStr;

    use crate::primitive::chip::{
        ChipColors,
        spans,
        speed_chip_spans,
        speed_chip_width,
    };

    fn colors() -> ChipColors {
        ChipColors {
            border: Color::DarkGray,
            value: Color::Cyan,
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
        let text = spans.as_ref().map(|spans| {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });
        assert_eq!(text.as_deref(), expected);

        let rendered: usize = spans
            .iter()
            .flatten()
            .map(|span| span.content.width())
            .sum();
        assert_eq!(
            usize::from(speed_chip_width(speed, mode)),
            rendered,
            "the reserved width must match what is painted"
        );
    }

    #[test]
    fn the_marker_and_the_value_carry_the_dim_face_and_their_own_colours() {
        let spans =
            speed_chip_spans(Speed::clamped(1.25), SpeedChip::Changed, colors())
                .unwrap_or_default();
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
        let colors = ChipColors {
            border: Color::White,
            value: Color::Red,
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
