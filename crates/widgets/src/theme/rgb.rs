use kernel::domain::appearance::Rgb;
use ratatui::style::Color;

use crate::pixels::numeric::channel_byte;

fn scale_channel(channel: u8, factor: f32) -> u8 {
    channel_byte(f32::from(channel) * factor)
}

pub(crate) fn shade(color: Rgb, factor: f32) -> Rgb {
    Rgb(color.0.map(|channel| scale_channel(channel, factor)))
}

fn lerp_channel(from: u8, to: u8, fraction: f32) -> u8 {
    channel_byte(f32::from(from) + (f32::from(to) - f32::from(from)) * fraction)
}

pub fn lerp_rgb(from: Rgb, to: Rgb, fraction: f32) -> Rgb {
    let fraction = fraction.clamp(0.0, 1.0);
    Rgb([
        lerp_channel(from.0[0], to.0[0], fraction),
        lerp_channel(from.0[1], to.0[1], fraction),
        lerp_channel(from.0[2], to.0[2], fraction),
    ])
}

pub(crate) fn gradient_at(stops: &[Rgb; 3], fraction: f32) -> Rgb {
    let [start, middle, end] = *stops;
    let scaled = fraction.clamp(0.0, 1.0) * 2.0;
    if scaled < 1.0 {
        lerp_rgb(start, middle, scaled)
    } else {
        lerp_rgb(middle, end, scaled - 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Indexed256,
}

impl ColorDepth {
    #[must_use]
    pub fn from_term_program(term_program: Option<&str>) -> Self {
        if term_program == Some("Apple_Terminal") {
            ColorDepth::Indexed256
        } else {
            ColorDepth::TrueColor
        }
    }
}

#[must_use]
pub fn color_at_depth(rgb: Rgb, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::TrueColor => Color::Rgb(rgb.0[0], rgb.0[1], rgb.0[2]),
        ColorDepth::Indexed256 => Color::Indexed(nearest_xterm256(rgb)),
    }
}

fn squared_error(sample: Rgb, target: Rgb) -> i32 {
    let [sample_r, sample_g, sample_b] = sample.0;
    let [target_r, target_g, target_b] = target.0;
    (i32::from(sample_r) - i32::from(target_r)).pow(2)
        + (i32::from(sample_g) - i32::from(target_g)).pow(2)
        + (i32::from(sample_b) - i32::from(target_b)).pow(2)
}

fn index_u8(channel: i32) -> u8 {
    u8::try_from(channel).unwrap_or(u8::MAX)
}

fn small_index_i32(step: usize) -> i32 {
    i32::try_from(step).unwrap_or(i32::MAX)
}

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
const CUBE_BASE: i32 = 16;
const CUBE_R_STRIDE: i32 = 36;
const CUBE_G_STRIDE: i32 = 6;
const GRAY_BASE: i32 = 232;
const GRAY_STEP: i32 = 10;
const GRAY_OFFSET: i32 = 8;
const GRAY_MAX_STEP: i32 = 23;

#[must_use]
fn nearest_xterm256(rgb: Rgb) -> u8 {
    let [r, g, b] = rgb.0;
    let nearest = |v: u8| -> (usize, u8) {
        CUBE_LEVELS
            .iter()
            .copied()
            .enumerate()
            .min_by_key(|&(_, level)| (i32::from(v) - i32::from(level)).abs())
            .unwrap_or((0, CUBE_LEVELS[0]))
    };
    let [(ri, rv), (gi, gv), (bi, bv)] = rgb.0.map(nearest);
    let cube_index = CUBE_BASE
        + CUBE_R_STRIDE * small_index_i32(ri)
        + CUBE_G_STRIDE * small_index_i32(gi)
        + small_index_i32(bi);
    let cube_err = squared_error(rgb, Rgb([rv, gv, bv]));

    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let gray_step = ((avg - GRAY_OFFSET).max(0) / GRAY_STEP).min(GRAY_MAX_STEP);
    let gray_level = GRAY_OFFSET + GRAY_STEP * gray_step;
    let gray_index = GRAY_BASE + gray_step;
    let gray_channel = u8::try_from(gray_level).unwrap_or(u8::MAX);
    let gray_err = squared_error(rgb, Rgb([gray_channel, gray_channel, gray_channel]));

    if gray_err < cube_err {
        index_u8(gray_index)
    } else {
        index_u8(cube_index)
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;
    use ratatui::style::Color;
    use rstest::rstest;

    use crate::theme::rgb::{
        ColorDepth,
        color_at_depth,
        gradient_at,
        lerp_rgb,
        nearest_xterm256,
        shade,
        squared_error,
    };

    const RAMP: [Rgb; 3] = [Rgb([0, 0, 0]), Rgb([128, 128, 128]), Rgb([255, 255, 255])];

    #[rstest]
    #[case::apple_terminal(Some("Apple_Terminal"), ColorDepth::Indexed256)]
    #[case::nothing_set(None, ColorDepth::TrueColor)]
    fn from_term_program_reads_the_terminals_identity_first(
        #[case] program: Option<&str>,
        #[case] depth: ColorDepth,
    ) {
        assert_eq!(ColorDepth::from_term_program(program), depth);
    }

    #[rstest]
    #[case::white([255, 255, 255], 231)]
    #[case::red([255, 0, 0], 196)]
    #[case::mid_grey([128, 128, 128], 244)]
    #[case::grey_as_near_the_cube_as_the_ramp([4, 4, 4], 16)]
    fn nearest_xterm256_picks_the_closest_index(
        #[case] rgb: [u8; 3],
        #[case] xterm_index: u8,
    ) {
        assert_eq!(nearest_xterm256(Rgb(rgb)), xterm_index);
    }

    #[rstest]
    #[case::truecolor(ColorDepth::TrueColor, Color::Rgb(0x2a, 0xa8, 0xa0))]
    #[case::indexed(ColorDepth::Indexed256, Color::Indexed(nearest_xterm256(Rgb([0x2a, 0xa8, 0xa0]))))]
    fn color_at_depth_answers_in_the_terminals_own_depth(
        #[case] depth: ColorDepth,
        #[case] expected: Color,
    ) {
        assert_eq!(color_at_depth(Rgb([0x2a, 0xa8, 0xa0]), depth), expected);
    }

    #[rstest]
    #[case::past_the_end(2.0, [100, 200, 255])]
    fn lerp_rgb_walks_between_two_colours(
        #[case] fraction: f32,
        #[case] expected: [u8; 3],
    ) {
        let start = Rgb([0, 0, 0]);
        let end = Rgb([100, 200, 255]);
        assert_eq!(lerp_rgb(start, end, fraction), Rgb(expected));
    }

    #[rstest]
    #[case::three_stops_inside_the_first_segment(0.25, Rgb([64, 64, 64]))]
    #[case::three_stops_inside_the_second_segment(0.75, Rgb([191, 191, 191]))]
    #[case::three_stops_at_the_end(1.0, Rgb([255, 255, 255]))]
    fn palette_at_samples_the_segment_t_falls_in(
        #[case] fraction: f32,
        #[case] expected: Rgb,
    ) {
        assert_eq!(gradient_at(&RAMP, fraction), expected);
    }

    #[rstest]
    #[case::half([200, 100, 0], 0.5, [100, 50, 0])]
    fn shade_scales_every_channel_by_the_factor(
        #[case] color: [u8; 3],
        #[case] factor: f32,
        #[case] expected: [u8; 3],
    ) {
        assert_eq!(shade(Rgb(color), factor), Rgb(expected));
    }

    #[rstest]
    #[case::every_channel_apart([1, 2, 3], [0, 0, 0], 14)]
    fn squared_error_sums_the_squared_channel_gaps(
        #[case] sample: [u8; 3],
        #[case] target: [u8; 3],
        #[case] expected: i32,
    ) {
        assert_eq!(squared_error(Rgb(sample), Rgb(target)), expected);
    }
}
