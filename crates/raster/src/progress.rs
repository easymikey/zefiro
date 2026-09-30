use config::{ProgressConfig, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarColorOverrides {
    pub fill: Option<Rgb>,
    pub track: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarColors {
    pub fill: Rgb,
    pub trough: Rgb,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgressGeometry {
    pub fraction: f32,
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
