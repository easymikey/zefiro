use num_traits::{Bounded, NumCast, ToPrimitive, Unsigned};

fn saturate<T: Unsigned + Bounded + NumCast>(scalar: f32) -> T {
    if scalar.is_nan() || scalar <= 0.0 {
        T::zero()
    } else {
        T::from(scalar).unwrap_or_else(T::max_value)
    }
}

#[inline]
#[must_use]
pub(crate) fn floor<T: Unsigned + Bounded + NumCast>(scalar: f32) -> T {
    saturate(scalar.floor())
}

#[inline]
#[must_use]
pub(crate) fn round<T: Unsigned + Bounded + NumCast>(scalar: f32) -> T {
    saturate(scalar.round())
}

#[inline]
#[must_use]
pub(crate) fn ceil<T: Unsigned + Bounded + NumCast>(scalar: f32) -> T {
    saturate(scalar.ceil())
}

#[inline]
#[must_use]
pub(crate) fn unit_fraction(scalar: f64) -> f32 {
    let clamped = if scalar.is_nan() {
        0.0
    } else {
        scalar.clamp(0.0, 1.0)
    };
    clamped.to_f32().unwrap_or(0.0)
}

#[inline]
#[must_use]
pub(crate) fn channel_byte(scalar: f32) -> u8 {
    if scalar.is_nan() {
        return 0;
    }
    scalar.clamp(0.0, 255.0).to_u8().unwrap_or(u8::MAX)
}

#[inline]
#[must_use]
pub(crate) fn dimension_f32<T: ToPrimitive + Copy>(count: T) -> f32 {
    count.to_f32().unwrap_or(f32::MAX)
}

#[inline]
#[must_use]
pub(crate) fn small_count_u16(count: impl TryInto<u16>) -> u16 {
    count.try_into().unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::pixels::numeric::{
        ceil,
        channel_byte,
        dimension_f32,
        floor,
        round,
        unit_fraction,
    };

    struct RoundingRow {
        to_u32: fn(f32) -> u32,
        to_usize: fn(f32) -> usize,
        scalar: f32,
        expected: u8,
    }

    #[rstest]
    #[case::floor_rounds_down(RoundingRow {
        to_u32: floor::<u32>,
        to_usize: floor::<usize>,
        scalar: 3.9,
        expected: 3,
    })]
    #[case::round_rounds_to_nearest(RoundingRow {
        to_u32: round::<u32>,
        to_usize: round::<usize>,
        scalar: 3.5,
        expected: 4,
    })]
    #[case::ceil_rounds_up(RoundingRow {
        to_u32: ceil::<u32>,
        to_usize: ceil::<usize>,
        scalar: 3.1,
        expected: 4,
    })]
    fn rounding_lands_on_a_whole_count_and_clamps_negatives(
        #[case] rounding_row: RoundingRow,
    ) {
        let RoundingRow {
            to_u32,
            to_usize,
            scalar,
            expected,
        } = rounding_row;
        assert_eq!(to_u32(scalar), u32::from(expected));
        assert_eq!(to_usize(scalar), usize::from(expected));
        assert_eq!(to_u32(-1.5), 0);
    }

    #[rstest]
    #[case::floor(floor::<u32>, floor::<u8>)]
    #[case::ceil(ceil::<u32>, ceil::<u8>)]
    fn rounding_saturates_on_nan_and_overflow(
        #[case] to_u32: fn(f32) -> u32,
        #[case] to_u8: fn(f32) -> u8,
    ) {
        assert_eq!(to_u32(f32::NAN), 0);
        assert_eq!(to_u8(1000.0), u8::MAX);
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
