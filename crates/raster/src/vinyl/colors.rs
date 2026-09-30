use config::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleeveFace {
    Blank,
    Art,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylColors {
    pub paper: Rgb,
    pub border: Rgb,
    pub record: Rgb,
    pub groove: Rgb,
    pub accent: Rgb,
    pub shadow: Rgb,
    pub blank_paper: Rgb,
}

impl Default for VinylColors {
    fn default() -> Self {
        Self {
            paper: Rgb([0xec, 0xe6, 0xd6]),
            border: Rgb([0x3a, 0x3a, 0x3a]),
            record: Rgb([0x10, 0x10, 0x10]),
            groove: Rgb([0xff, 0xff, 0xff]),
            accent: Rgb([0xff, 0x6b, 0x3d]),
            shadow: Rgb([0x00, 0x00, 0x00]),
            blank_paper: Rgb([0x1a, 0x1a, 0x1a]),
        }
    }
}

pub(crate) fn scale_alpha(peak: u8, fraction: f32) -> u8 {
    crate::numeric::channel_byte(
        (crate::numeric::dimension_f32(u32::from(peak)) * fraction).round(),
    )
}
