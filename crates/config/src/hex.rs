use std::{fmt, str::FromStr};

use serde::Deserialize;

use crate::error::ColorError;

#[must_use]
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String")]
pub struct Rgb(pub [u8; 3]);

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte.to_ascii_lowercase() {
        digit @ b'0'..=b'9' => Some(digit - b'0'),
        letter @ b'a'..=b'f' => Some(letter - b'a' + 10),
        _ => None,
    }
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    Some(hex_nibble(high)? * 16 + hex_nibble(low)?)
}

fn parse_hex(spelling: &str) -> Result<Rgb, ColorError> {
    let trimmed = spelling.strip_prefix('#').unwrap_or(spelling);
    let sized = <[u8; 6]>::try_from(trimmed.as_bytes()).ok();
    sized
        .and_then(|[r1, r0, g1, g0, b1, b0]| {
            Some(Rgb([
                hex_byte(r1, r0)?,
                hex_byte(g1, g0)?,
                hex_byte(b1, b0)?,
            ]))
        })
        .ok_or_else(|| ColorError {
            input: spelling.to_string(),
        })
}

impl TryFrom<String> for Rgb {
    type Error = ColorError;

    fn try_from(spelling: String) -> Result<Self, Self::Error> {
        parse_hex(&spelling)
    }
}

impl FromStr for Rgb {
    type Err = ColorError;

    fn from_str(spelling: &str) -> Result<Self, Self::Err> {
        parse_hex(spelling)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(formatter, "#{r:02x}{g:02x}{b:02x}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::hex::Rgb;

    #[rstest]
    #[case::with_hash("#2aa8a0", [0x2a, 0xa8, 0xa0])]
    #[case::without_hash("2aa8a0", [0x2a, 0xa8, 0xa0])]
    fn parse_accepts_valid_six_digit_hex(#[case] input: &str, #[case] want: [u8; 3]) {
        match Rgb::try_from(input.to_string()) {
            Ok(hex) => assert_eq!(hex, Rgb(want)),
            Err(error) => panic!("expected {input:?} to parse, got {error}"),
        }
    }

    #[rstest]
    #[case::too_short("#fff")]
    #[case::too_long("#ffffffff")]
    #[case::empty("")]
    #[case::non_hex_digit("#ff00zz")]
    fn parse_rejects_invalid_input(#[case] input: &str) {
        assert!(Rgb::try_from(input.to_string()).is_err());
    }

    #[test]
    fn from_str_and_try_from_string_agree() {
        let want = Rgb::try_from("#112233".to_string()).unwrap();
        assert_eq!("#112233".parse::<Rgb>().unwrap(), want);
    }

    #[test]
    fn display_spells_the_hex_back_with_a_hash_and_lowercase_digits() {
        assert_eq!(Rgb([0x2a, 0xa8, 0xf0]).to_string(), "#2aa8f0");
    }
}
