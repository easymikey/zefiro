use num_traits::ToPrimitive;

#[inline]
#[must_use]
pub fn floor_u32(value: f32) -> u32 {
    let floored = value.floor();
    if floored.is_nan() {
        return 0;
    }
    if floored <= 0.0 {
        0
    } else {
        floored.to_u32().unwrap_or(u32::MAX)
    }
}

#[inline]
#[must_use]
pub fn round_u32(value: f32) -> u32 {
    let rounded = value.round();
    if rounded.is_nan() {
        return 0;
    }
    if rounded <= 0.0 {
        0
    } else {
        rounded.to_u32().unwrap_or(u32::MAX)
    }
}

#[inline]
#[must_use]
pub fn floor_usize(value: f32) -> usize {
    let floored = value.floor();
    if floored.is_nan() {
        return 0;
    }
    if floored <= 0.0 {
        0
    } else {
        floored.to_usize().unwrap_or(usize::MAX)
    }
}

#[inline]
#[must_use]
pub fn round_usize(value: f32) -> usize {
    let rounded = value.round();
    if rounded.is_nan() {
        return 0;
    }
    if rounded <= 0.0 {
        0
    } else {
        rounded.to_usize().unwrap_or(usize::MAX)
    }
}

#[inline]
#[must_use]
pub(crate) fn round_i32(value: f32) -> i32 {
    let rounded = value.round();
    if rounded.is_nan() {
        return 0;
    }
    if rounded.is_sign_negative() {
        rounded.to_i32().unwrap_or(i32::MIN)
    } else {
        rounded.to_i32().unwrap_or(i32::MAX)
    }
}

#[inline]
#[must_use]
pub fn unit_fraction(value: f64) -> f32 {
    let clamped = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    clamped.to_f32().unwrap_or(0.0)
}

#[inline]
#[must_use]
pub fn channel_byte(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(0.0, 255.0).to_u8().unwrap_or(u8::MAX)
}

#[inline]
#[must_use]
pub fn dimension_f32(value: u32) -> f32 {
    value.to_f32().unwrap_or(f32::MAX)
}

#[inline]
#[must_use]
pub fn dimension_u32(value: f32) -> u32 {
    floor_u32(value)
}

#[cfg(test)]
mod tests {
    use crate::numeric::{
        channel_byte,
        dimension_f32,
        dimension_u32,
        floor_u32,
        floor_usize,
        round_i32,
        round_u32,
        round_usize,
        unit_fraction,
    };

    #[test]
    fn floor_u32_rounds_down_and_clamps_negatives() {
        assert_eq!(floor_u32(3.9), 3);
        assert_eq!(floor_u32(-1.5), 0);
    }

    #[test]
    fn round_u32_rounds_to_nearest_and_clamps_negatives() {
        assert_eq!(round_u32(3.5), 4);
        assert_eq!(round_u32(-1.5), 0);
    }

    #[test]
    fn floor_usize_rounds_down_and_clamps_negatives() {
        assert_eq!(floor_usize(3.9), 3);
        assert_eq!(floor_usize(-1.5), 0);
    }

    #[test]
    fn round_usize_rounds_to_nearest_and_clamps_negatives() {
        assert_eq!(round_usize(3.5), 4);
        assert_eq!(round_usize(-1.5), 0);
    }

    #[test]
    fn round_i32_rounds_to_nearest() {
        assert_eq!(round_i32(3.5), 4);
        assert_eq!(round_i32(-3.5), -4);
    }

    #[test]
    fn unit_fraction_clamps_to_zero_one() {
        assert_eq!(unit_fraction(-1.0), 0.0);
        assert_eq!(unit_fraction(0.5), 0.5);
        assert_eq!(unit_fraction(2.0), 1.0);
    }

    #[test]
    fn channel_byte_clamps_to_a_u8() {
        assert_eq!(channel_byte(-10.0), 0);
        assert_eq!(channel_byte(300.0), 255);
    }

    #[test]
    fn dimension_f32_converts_a_pixel_count_without_loss() {
        assert_eq!(dimension_f32(100), 100.0);
    }

    #[test]
    fn dimension_u32_floors_and_clamps_negatives() {
        assert_eq!(dimension_u32(3.9), 3);
        assert_eq!(dimension_u32(-1.5), 0);
    }
}
