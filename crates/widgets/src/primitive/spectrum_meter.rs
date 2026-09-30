use ratatui::{style::Color, text::Line};

use crate::{
    braille,
    primitive::span::{line, text},
};

fn spectrum_row_fraction(row: u16, total_rows: u16) -> f32 {
    if total_rows <= 1 {
        return 1.0;
    }
    1.0 - f32::from(row) / f32::from(total_rows - 1)
}

#[must_use]
pub(crate) fn lines(
    fill: &braille::MeterFill<'_>,
    color_at: impl Fn(f32) -> Color,
    buffers: &mut braille::BrailleBuffers,
) -> Vec<Line<'static>> {
    let spectrum_rows = buffers.render_meter(fill);
    let spectrum_total_rows = u16::try_from(spectrum_rows.len()).unwrap_or(u16::MAX);
    spectrum_rows
        .iter()
        .enumerate()
        .map(|(row, glyphs)| {
            let t = spectrum_row_fraction(
                u16::try_from(row).unwrap_or(u16::MAX),
                spectrum_total_rows,
            );
            let color = color_at(t);
            line([text(glyphs.clone()).fg(color)])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::primitive::spectrum_meter::spectrum_row_fraction;

    #[rstest]
    #[case::top_row(0, 4, 1.0)]
    #[case::second_row(1, 4, 1.0 - 1.0f32 / 3.0f32)]
    #[case::bottom_row(3, 4, 0.0)]
    #[case::the_only_row(0, 1, 1.0)]
    #[case::no_rows_at_all(0, 0, 1.0)]
    fn spectrum_row_fraction_runs_high_at_the_top(
        #[case] row: u16,
        #[case] rows: u16,
        #[case] expected: f32,
    ) {
        assert_eq!(spectrum_row_fraction(row, rows), expected);
    }
}
