use num_traits::{Bounded, NumCast, ToPrimitive, Unsigned};

fn saturate<T: Unsigned + Bounded + NumCast>(value: f32) -> T {
    if value.is_nan() || value <= 0.0 {
        T::zero()
    } else {
        T::from(value).unwrap_or_else(T::max_value)
    }
}

#[inline]
#[must_use]
pub(crate) fn floor<T: Unsigned + Bounded + NumCast>(value: f32) -> T {
    saturate(value.floor())
}

#[inline]
#[must_use]
pub(crate) fn round<T: Unsigned + Bounded + NumCast>(value: f32) -> T {
    saturate(value.round())
}

#[inline]
#[must_use]
pub(crate) fn unit_fraction(value: f64) -> f32 {
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
pub(crate) fn dimension_f32(value: u32) -> f32 {
    value.to_f32().unwrap_or(f32::MAX)
}

#[cfg(test)]
mod tests {
    use crate::pixels::numeric::{
        channel_byte,
        dimension_f32,
        floor,
        round,
        unit_fraction,
    };

    #[test]
    fn floor_rounds_down_and_clamps_negatives() {
        assert_eq!(floor::<u32>(3.9), 3);
        assert_eq!(floor::<usize>(3.9), 3);
        assert_eq!(floor::<u32>(-1.5), 0);
    }

    #[test]
    fn round_rounds_to_nearest_and_clamps_negatives() {
        assert_eq!(round::<u32>(3.5), 4);
        assert_eq!(round::<usize>(3.5), 4);
        assert_eq!(round::<u32>(-1.5), 0);
    }

    #[test]
    fn floor_saturates_on_nan_and_overflow() {
        assert_eq!(floor::<u32>(f32::NAN), 0);
        assert_eq!(floor::<u8>(1000.0), u8::MAX);
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
}
