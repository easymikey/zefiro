use kernel::domain::track::{AudioFormat, Hertz, Kbps};
use ratatui::{
    style::Color,
    text::{Line, Span},
};

use crate::{
    pixels::numeric::dimension_f32,
    primitive::{
        chip,
        glyphs::audio_format::{BITRATE_UNIT, SAMPLE_RATE_UNIT},
    },
    theme::colors::Colors,
};

const HZ_PER_KHZ: f32 = 1000.0;

#[must_use]
fn kilohertz(Hertz(sample_rate): Hertz) -> f32 {
    dimension_f32(sample_rate) / HZ_PER_KHZ
}

#[must_use]
pub(crate) fn format_chip_values(audio_format: &AudioFormat) -> Vec<String> {
    [
        audio_format.format.as_deref().map(str::to_uppercase),
        audio_format
            .bitrate
            .map(|Kbps(bitrate)| format!("{bitrate}{BITRATE_UNIT}")),
        audio_format.sample_rate.map(|sample_rate| {
            format!("{:.1}{SAMPLE_RATE_UNIT}", kilohertz(sample_rate))
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[must_use]
pub(crate) fn format_chips_line(
    audio_format: &AudioFormat,
    colors: &Colors<Color>,
    max_width: usize,
) -> Option<Line<'static>> {
    let values = format_chip_values(audio_format);
    let spans: Vec<Span<'static>> = values
        .into_iter()
        .map(|value| chip::spans(value, colors))
        .enumerate()
        .scan(0usize, |used_width, (index, chip_spans)| {
            let chip_width = crate::primitive::span::width(&chip_spans);
            *used_width += usize::from(index > 0) + chip_width;
            (*used_width <= max_width).then_some((index, chip_spans))
        })
        .flat_map(|(index, chip_spans)| {
            (index > 0)
                .then(|| Span::raw(" "))
                .into_iter()
                .chain(chip_spans)
        })
        .collect();
    (!spans.is_empty()).then(|| Line::from(spans))
}

#[cfg(test)]
mod tests {
    use kernel::domain::track::{AudioFormat, Hertz, Kbps};
    use ratatui::style::Color;
    use rstest::rstest;

    use crate::{primitive::format_chips::format_chips_line, theme::colors::Colors};

    fn colors() -> Colors<Color> {
        Colors {
            muted_foreground: Color::Gray,
            foreground: Color::White,
            ..Colors::default()
        }
    }

    const FULL: &str = "[ MP3 ] [ 320 KBPS ] [ 44.1 KHZ ]";

    fn full_audio_format() -> AudioFormat {
        AudioFormat {
            format: Some("mp3".to_string()),
            bitrate: Some(Kbps(320)),
            sample_rate: Some(Hertz(44100)),
            ..Default::default()
        }
    }

    fn bitrate_only() -> AudioFormat {
        AudioFormat {
            bitrate: Some(Kbps(128)),
            ..Default::default()
        }
    }

    #[rstest]
    #[case::nothing_known(AudioFormat::default(), usize::MAX, None)]
    #[case::nothing_known_with_room(AudioFormat::default(), 100, None)]
    #[case::one_fact(bitrate_only(), usize::MAX, Some("[ 128 KBPS ]"))]
    #[case::every_fact(full_audio_format(), usize::MAX, Some(FULL))]
    #[case::every_fact_with_room_to_spare(full_audio_format(), 100, Some(FULL))]
    #[case::one_cell_short(full_audio_format(), FULL.chars().count() - 1, Some("[ MP3 ] [ 320 KBPS ]"))]
    #[case::room_for_one_chip_and_a_little(
        full_audio_format(),
        "[ MP3 ]".chars().count() + 3,
        Some("[ MP3 ]")
    )]
    #[case::no_room_at_all(full_audio_format(), 2, None)]
    fn format_chips_line_drops_whole_chips_from_the_right(
        #[case] audio_format: AudioFormat,
        #[case] width: usize,
        #[case] expected: Option<&str>,
    ) {
        let line = format_chips_line(&audio_format, &colors(), width);
        assert_eq!(line.as_ref().map(ToString::to_string).as_deref(), expected);
    }
}
