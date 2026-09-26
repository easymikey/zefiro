use config::{Hex, ProgressConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarColorOverrides {
    pub fill: Option<Hex>,
    pub track: Option<Hex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarColors {
    pub fill: Hex,
    pub track: Hex,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgressGeometry {
    pub frac: f32,
    pub width: u32,
    pub height: u32,
}

#[must_use]
pub fn color_overrides(config: &ProgressConfig) -> BarColorOverrides {
    BarColorOverrides {
        fill: config.fill,
        track: config.track,
    }
}
