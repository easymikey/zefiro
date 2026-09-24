use config::Hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleeveFace {
    Blank,
    Art,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylColors {
    pub paper: Hex,
    pub border: Hex,
    pub record: Hex,
    pub groove: Hex,
    pub accent: Hex,
    pub shadow: Hex,
    pub blank_paper: Hex,
}

impl Default for VinylColors {
    fn default() -> Self {
        Self {
            paper: Hex([0xec, 0xe6, 0xd6]),
            border: Hex([0x3a, 0x3a, 0x3a]),
            record: Hex([0x10, 0x10, 0x10]),
            groove: Hex([0xff, 0xff, 0xff]),
            accent: Hex([0xff, 0x6b, 0x3d]),
            shadow: Hex([0x00, 0x00, 0x00]),
            blank_paper: Hex([0x1a, 0x1a, 0x1a]),
        }
    }
}

pub(crate) fn scale_alpha(peak: u8, fraction: f32) -> u8 {
    crate::numeric::channel_byte(
        (crate::numeric::dimension_f32(u32::from(peak)) * fraction).round(),
    )
}
