use config::Hex;
use num_traits::ToPrimitive;
use raster::{channel_byte, floor_usize};
use ratatui::style::Color;

pub(crate) fn scale_channel(value: u8, factor: f32) -> u8 {
    channel_byte(f32::from(value) * factor)
}

pub(crate) fn shade(color: Hex, factor: f32) -> Hex {
    Hex([
        scale_channel(color.0[0], factor),
        scale_channel(color.0[1], factor),
        scale_channel(color.0[2], factor),
    ])
}

fn lerp_channel(a: u8, b: u8, t: f32) -> u8 {
    channel_byte(f32::from(a) + (f32::from(b) - f32::from(a)) * t)
}

pub fn lerp_rgb(a: Hex, b: Hex, t: f32) -> Hex {
    let t = t.clamp(0.0, 1.0);
    Hex([
        lerp_channel(a.0[0], b.0[0], t),
        lerp_channel(a.0[1], b.0[1], t),
        lerp_channel(a.0[2], b.0[2], t),
    ])
}

#[must_use]
pub(crate) fn palette_at(stops: &[Hex], t: f32) -> Option<Hex> {
    match stops.len() {
        0 => None,
        1 => stops.first().copied(),
        count => {
            let t = t.clamp(0.0, 1.0);
            let segments = (count - 1).to_f32().unwrap_or(f32::MAX);
            let scaled = t * segments;
            let index = floor_usize(scaled).min(count - 2);
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

#[must_use]
pub fn detect(term_program: Option<&str>, _colorterm: Option<&str>) -> ColorDepth {
    if term_program == Some("Apple_Terminal") {
        ColorDepth::Indexed256
    } else {
        ColorDepth::TrueColor
    }
}

#[must_use]
pub fn color_at_depth(hex: Hex, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::TrueColor => Color::Rgb(hex.0[0], hex.0[1], hex.0[2]),
        ColorDepth::Indexed256 => Color::Indexed(nearest_xterm256(hex)),
    }
}

fn channel_sq_err(sample: Hex, target: Hex) -> i32 {
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
fn nearest_xterm256(hex: Hex) -> u8 {
    let [r, g, b] = hex.0;
    let nearest = |v: u8| -> (usize, u8) {
        CUBE_LEVELS
            .iter()
            .copied()
            .enumerate()
            .min_by_key(|&(_, level)| (i32::from(v) - i32::from(level)).abs())
            .unwrap_or((0, CUBE_LEVELS[0]))
    };
    let [(ri, rv), (gi, gv), (bi, bv)] = hex.0.map(nearest);
    let cube_index = CUBE_BASE
        + CUBE_R_STRIDE * small_index_i32(ri)
        + CUBE_G_STRIDE * small_index_i32(gi)
        + small_index_i32(bi);
    let cube_err = channel_sq_err(hex, Hex([rv, gv, bv]));

    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let gray_step = ((avg - GRAY_OFFSET).max(0) / GRAY_STEP).min(GRAY_MAX_STEP);
    let gray_level = GRAY_OFFSET + GRAY_STEP * gray_step;
    let gray_index = GRAY_BASE + gray_step;
    let gray_channel = u8::try_from(gray_level).unwrap_or(u8::MAX);
    let gray_err = channel_sq_err(hex, Hex([gray_channel, gray_channel, gray_channel]));

    if gray_err < cube_err {
        index_u8(gray_index)
    } else {
        index_u8(cube_index)
    }
}

#[cfg(test)]
mod tests {
    use config::Hex;
    use ratatui::style::Color;
    use rstest::rstest;

    use crate::theme::hex::{
        ColorDepth,
        color_at_depth,
        detect,
        lerp_rgb,
        nearest_xterm256,
        palette_at,
    };

    const RAMP: &[Hex] = &[Hex([0, 0, 0]), Hex([128, 128, 128]), Hex([255, 255, 255])];

    #[rstest]
    #[case::apple_terminal(Some("Apple_Terminal"), None, ColorDepth::Indexed256)]
    #[case::apple_terminal_claiming_truecolor(
        Some("Apple_Terminal"),
        Some("truecolor"),
        ColorDepth::Indexed256
    )]
    #[case::ghostty(Some("ghostty"), None, ColorDepth::TrueColor)]
    #[case::iterm2(Some("iTerm.app"), Some("truecolor"), ColorDepth::TrueColor)]
    #[case::colorterm_alone(None, Some("24bit"), ColorDepth::TrueColor)]
    #[case::nothing_set(None, None, ColorDepth::TrueColor)]
    fn detect_reads_the_terminals_identity_first(
        #[case] program: Option<&str>,
        #[case] colorterm: Option<&str>,
        #[case] depth: ColorDepth,
    ) {
        assert_eq!(detect(program, colorterm), depth);
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
        assert_eq!(nearest_xterm256(Hex(rgb)), index);
    }

    #[rstest]
    #[case::truecolor(ColorDepth::TrueColor, Color::Rgb(0x2a, 0xa8, 0xa0))]
    #[case::indexed(ColorDepth::Indexed256, Color::Indexed(nearest_xterm256(Hex([0x2a, 0xa8, 0xa0]))))]
    fn color_at_depth_answers_in_the_terminals_own_depth(
        #[case] depth: ColorDepth,
        #[case] expected: Color,
    ) {
        assert_eq!(color_at_depth(Hex([0x2a, 0xa8, 0xa0]), depth), expected);
    }

    #[rstest]
    #[case::at_the_start(0.0, [0, 0, 0])]
    #[case::at_the_midpoint(0.5, [50, 100, 127])]
    #[case::at_the_end(1.0, [100, 200, 255])]
    #[case::before_the_start(-1.0, [0, 0, 0])]
    #[case::past_the_end(2.0, [100, 200, 255])]
    fn lerp_rgb_walks_between_two_colours(#[case] t: f32, #[case] expected: [u8; 3]) {
        let start = Hex([0, 0, 0]);
        let end = Hex([100, 200, 255]);
        assert_eq!(lerp_rgb(start, end, t), Hex(expected));
    }

    #[rstest]
    #[case::no_stops(&[], 0.5, None)]
    #[case::one_stop_at_the_start(&[Hex([1, 2, 3])], 0.0, Some(Hex([1, 2, 3])))]
    #[case::one_stop_in_the_middle(&[Hex([1, 2, 3])], 0.5, Some(Hex([1, 2, 3])))]
    #[case::one_stop_at_the_end(&[Hex([1, 2, 3])], 1.0, Some(Hex([1, 2, 3])))]
    #[case::three_stops_at_the_start(RAMP, 0.0, Some(Hex([0, 0, 0])))]
    #[case::three_stops_inside_the_first_segment(RAMP, 0.25, Some(Hex([64, 64, 64])))]
    #[case::three_stops_inside_the_second_segment(RAMP, 0.75, Some(Hex([191, 191, 191])))]
    #[case::three_stops_at_the_end(RAMP, 1.0, Some(Hex([255, 255, 255])))]
    #[case::before_the_start(RAMP, -1.0, Some(Hex([0, 0, 0])))]
    #[case::past_the_end(RAMP, 2.0, Some(Hex([255, 255, 255])))]
    fn palette_at_samples_the_segment_t_falls_in(
        #[case] stops: &[Hex],
        #[case] t: f32,
        #[case] expected: Option<Hex>,
    ) {
        assert_eq!(palette_at(stops, t), expected);
    }
}
