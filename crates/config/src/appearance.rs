use std::{fmt::Display, str::FromStr};

pub use kernel::domain::appearance::{
    Animations,
    AppearancePatch,
    AppearancePreset,
    AppearanceSettings,
    CoverBrackets,
    CoverMode,
    FormatChips,
    KeyHints,
    LayoutMode,
    ProgressTime,
    Rgb,
    SpeedChip,
    preset_of,
};
use serde::{Deserialize, Deserializer, de::Error as _};
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
