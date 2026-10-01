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

/// A cover already decoded to pixels, handed in from outside the crate.
#[derive(Debug, Clone)]
pub struct DecodedCover {
    pub path: PathBuf,
    pub image: Arc<RgbaImage>,
}

/// Whether the plain cover is mid-crossfade, for callers that must keep
/// asking for animation frames while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverMotion {
    Animating,
    Still,
}

/// Whether the caller grants the plain cover permission to begin a
/// crossfade when it installs a new pixmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadePermit {
    Allowed,
    Withheld,
}

/// The screen's theme wash, for a cover that must keep showing its outgoing
/// theme's colours behind the reveal until the wash catches up with it.
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
