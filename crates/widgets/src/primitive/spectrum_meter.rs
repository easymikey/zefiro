use ratatui::style::Color;

use crate::{
    braille::{BrailleCanvas, MeterFill},
    primitive::canvas::Canvas,
};

fn spectrum_row_fraction(y: u16, total_rows: u16) -> f32 {
    if total_rows <= 1 {
        return 1.0;
    }
    1.0 - f32::from(y) / f32::from(total_rows - 1)
}

pub(crate) fn paint(
    fill: &MeterFill<'_>,
    canvas: Canvas<'_>,
    color_at: impl Fn(f32) -> Color,
) {
    let total_rows = fill.size.height;
    BrailleCanvas::from(fill).paint(canvas, |row| {
        color_at(spectrum_row_fraction(row, total_rows))
    });
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::primitive::spectrum_meter::spectrum_row_fraction;

    #[rstest]
    #[case::second_row(1, 4, 1.0 - 1.0f32 / 3.0f32)]
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
