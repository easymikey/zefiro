use std::{fmt::Display, str::FromStr};

use kernel::domain::{
    appearance::{Animations, CoverBrackets, FormatChips, KeyHints, ProgressTime},
    geometry::Pixels,
};
use num_traits::ToPrimitive;
use serde::{
    Deserialize,
    Deserializer,
    de::{Error as _, Unexpected},
};
use strum::VariantNames;

pub(crate) trait Flag {
    const ON: Self;
    const OFF: Self;
}

pub(crate) fn flag<'de, D, F>(deserializer: D) -> Result<F, D::Error>
where
    D: Deserializer<'de>,
    F: Flag,
{
    Ok(if bool::deserialize(deserializer)? {
        F::ON
    } else {
        F::OFF
    })
}

pub(crate) fn from_str_field<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(D::Error::custom)
}

pub(crate) fn variant_field<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + VariantNames,
{
    let spelling = String::deserialize(deserializer)?;
    spelling
        .parse()
        .map_err(|_| D::Error::unknown_variant(&spelling, T::VARIANTS))
}

pub(crate) fn rounded_pixels<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: From<Pixels>,
{
    let raw = f32::deserialize(deserializer)?;
    raw.round()
        .to_u32()
        .map(Pixels)
        .map(T::from)
        .ok_or_else(|| {
            D::Error::invalid_value(
                Unexpected::Float(f64::from(raw)),
                &"a non-negative number of pixels",
            )
        })
}

pub(crate) fn from_str_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    Option::<String>::deserialize(deserializer)?
        .map(|spelling| spelling.parse().map_err(D::Error::custom))
        .transpose()
}

impl Flag for CoverBrackets {
    const ON: Self = Self::Shown;
    const OFF: Self = Self::Hidden;
}

impl Flag for FormatChips {
    const ON: Self = Self::Shown;
    const OFF: Self = Self::Hidden;
}

impl Flag for ProgressTime {
    const ON: Self = Self::Remaining;
    const OFF: Self = Self::Elapsed;
}

impl Flag for Animations {
    const ON: Self = Self::On;
    const OFF: Self = Self::Off;
}

impl Flag for KeyHints {
    const ON: Self = Self::Shown;
    const OFF: Self = Self::Hidden;
}
