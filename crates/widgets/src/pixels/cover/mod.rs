mod crossfade;
mod lifecycle;
mod milkdrop;
mod pixmap;
mod wash;

use std::{path::PathBuf, sync::Arc};

use image::RgbaImage;

use crate::FrameLayout;
pub use crate::pixels::cover::{
    crossfade::{CoverCrossfade, CrossfadeStage, blend_by_column},
    lifecycle::{CoverFrame, CoverLifecycle, CoverUpdate, PixmapSource},
    milkdrop::MilkdropCover,
    pixmap::CellPixels,
    wash::column_reveal,
};

#[derive(Debug, Clone)]
pub struct DecodedCover {
    pub path: PathBuf,
    pub image: Arc<RgbaImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverMotion {
    Animating,
    Still,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadePermit {
    Allowed,
    Withheld,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverWash {
    Running { progress: f32, screen_width: u16 },
    Idle,
}

#[derive(Debug, Clone, Copy)]
pub struct CoverRefreshParts {
    pub layout: FrameLayout,
    pub crossfade: CrossfadePermit,
    pub wash: CoverWash,
}
