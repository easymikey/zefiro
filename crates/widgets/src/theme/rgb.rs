use config::Rgb;
use num_traits::ToPrimitive;
use ratatui::style::Color;

use crate::pixels::{channel_byte, floor};

pub(crate) fn scale_channel(value: u8, factor: f32) -> u8 {
    channel_byte(f32::from(value) * factor)
}

pub fn shade(color: Rgb, factor: f32) -> Rgb {
    Rgb([
        scale_channel(color.0[0], factor),
        scale_channel(color.0[1], factor),
        scale_channel(color.0[2], factor),
    ])
}

fn lerp_channel(a: u8, b: u8, t: f32) -> u8 {
    channel_byte(f32::from(a) + (f32::from(b) - f32::from(a)) * t)
}

pub fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    Rgb([
        lerp_channel(a.0[0], b.0[0], t),
        lerp_channel(a.0[1], b.0[1], t),
        lerp_channel(a.0[2], b.0[2], t),
    ])
}

#[must_use]
pub(crate) fn gradient_at(stops: &[Rgb], t: f32) -> Option<Rgb> {
    match stops.len() {
        0 => None,
        1 => stops.first().copied(),
        count => {
            let t = t.clamp(0.0, 1.0);
            let segments = (count - 1).to_f32().unwrap_or(f32::MAX);
            let scaled = t * segments;
            let index = floor::<usize>(scaled).min(count - 2);
            let local_t = scaled - index.to_f32().unwrap_or(f32::MAX);
            stops
                .get(index)
                .zip(stops.get(index + 1))
                .map(|(&a, &b)| lerp_rgb(a, b, local_t))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Indexed256,
}

impl ColorDepth {
    #[must_use]
    pub fn detect(term_program: Option<&str>) -> Self {
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

fn channel_sq_err(sample: Rgb, target: Rgb) -> i32 {
    let [sample_r, sample_g, sample_b] = sample.0;
    let [target_r, target_g, target_b] = target.0;
    (i32::from(sample_r) - i32::from(target_r)).pow(2)
        + (i32::from(sample_g) - i32::from(target_g)).pow(2)
        + (i32::from(sample_b) - i32::from(target_b)).pow(2)
}

fn index_u8(v: i32) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
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
    let cube_err = channel_sq_err(rgb, Rgb([rv, gv, bv]));

    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let gray_step = ((avg - GRAY_OFFSET).max(0) / GRAY_STEP).min(GRAY_MAX_STEP);
    let gray_level = GRAY_OFFSET + GRAY_STEP * gray_step;
    let gray_index = GRAY_BASE + gray_step;
    let gray_channel = u8::try_from(gray_level).unwrap_or(u8::MAX);
    let gray_err = channel_sq_err(rgb, Rgb([gray_channel, gray_channel, gray_channel]));

    if gray_err < cube_err {
        index_u8(gray_index)
    } else {
        index_u8(cube_index)
    }
}

#[cfg(test)]
mod tests {
    use config::Rgb;
    use ratatui::style::Color;
    use rstest::rstest;

    use crate::theme::rgb::{
        ColorDepth,
        color_at_depth,
        gradient_at,
        lerp_rgb,
        nearest_xterm256,
    };

    const RAMP: &[Rgb] = &[Rgb([0, 0, 0]), Rgb([128, 128, 128]), Rgb([255, 255, 255])];

    #[rstest]
    #[case::apple_terminal(Some("Apple_Terminal"), ColorDepth::Indexed256)]
    #[case::ghostty(Some("ghostty"), ColorDepth::TrueColor)]
    #[case::iterm2(Some("iTerm.app"), ColorDepth::TrueColor)]
    #[case::nothing_set(None, ColorDepth::TrueColor)]
    fn detect_reads_the_terminals_identity_first(
        #[case] program: Option<&str>,
        #[case] depth: ColorDepth,
    ) {
        assert_eq!(ColorDepth::detect(program), depth);
    }

    #[rstest]
    #[case::black([0, 0, 0], 16)]
    #[case::white([255, 255, 255], 231)]
    #[case::red([255, 0, 0], 196)]
    #[case::mid_grey([128, 128, 128], 244)]
    fn nearest_xterm256_picks_the_closest_index(
        #[case] rgb: [u8; 3],
        #[case] index: u8,
    ) {
        assert_eq!(nearest_xterm256(Rgb(rgb)), index);
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
    #[case::at_the_start(0.0, [0, 0, 0])]
    #[case::at_the_midpoint(0.5, [50, 100, 127])]
    #[case::at_the_end(1.0, [100, 200, 255])]
    #[case::before_the_start(-1.0, [0, 0, 0])]
    #[case::past_the_end(2.0, [100, 200, 255])]
    fn lerp_rgb_walks_between_two_colours(#[case] t: f32, #[case] expected: [u8; 3]) {
        let start = Rgb([0, 0, 0]);
        let end = Rgb([100, 200, 255]);
        assert_eq!(lerp_rgb(start, end, t), Rgb(expected));
    }

    #[rstest]
    #[case::no_stops(&[], 0.5, None)]
    #[case::one_stop_at_the_start(&[Rgb([1, 2, 3])], 0.0, Some(Rgb([1, 2, 3])))]
    #[case::one_stop_in_the_middle(&[Rgb([1, 2, 3])], 0.5, Some(Rgb([1, 2, 3])))]
    #[case::one_stop_at_the_end(&[Rgb([1, 2, 3])], 1.0, Some(Rgb([1, 2, 3])))]
    #[case::three_stops_at_the_start(RAMP, 0.0, Some(Rgb([0, 0, 0])))]
    #[case::three_stops_inside_the_first_segment(RAMP, 0.25, Some(Rgb([64, 64, 64])))]
    #[case::three_stops_inside_the_second_segment(RAMP, 0.75, Some(Rgb([191, 191, 191])))]
    #[case::three_stops_at_the_end(RAMP, 1.0, Some(Rgb([255, 255, 255])))]
    #[case::before_the_start(RAMP, -1.0, Some(Rgb([0, 0, 0])))]
    #[case::past_the_end(RAMP, 2.0, Some(Rgb([255, 255, 255])))]
    fn palette_at_samples_the_segment_t_falls_in(
        #[case] stops: &[Rgb],
        #[case] t: f32,
        #[case] expected: Option<Rgb>,
    ) {
        assert_eq!(gradient_at(stops, t), expected);
    }
}
