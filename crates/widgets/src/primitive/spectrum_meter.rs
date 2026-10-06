use ratatui::{style::Color, text::Line};

use crate::{
    braille,
    pixels::numeric::small_count_u16,
    primitive::span::{line, text},
};

fn spectrum_row_fraction(y: u16, total_rows: u16) -> f32 {
    if total_rows <= 1 {
        return 1.0;
    }
    1.0 - f32::from(y) / f32::from(total_rows - 1)
}

#[must_use]
pub(crate) fn lines(
    fill: &braille::MeterFill<'_>,
    color_at: impl Fn(f32) -> Color,
) -> Vec<Line<'static>> {
    let spectrum_rows = braille::meter_rows(fill);
    let spectrum_total_rows = small_count_u16(spectrum_rows.len());
    spectrum_rows
        .into_iter()
        .enumerate()
        .map(|(row, glyphs)| {
            let fraction =
                spectrum_row_fraction(small_count_u16(row), spectrum_total_rows);
            let color = color_at(fraction);
            line([text(glyphs).fg(color)])
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
        #[case] y: u16,
        #[case] rows: u16,
        #[case] expected: f32,
    ) {
        assert_eq!(spectrum_row_fraction(y, rows), expected);
    }
}
