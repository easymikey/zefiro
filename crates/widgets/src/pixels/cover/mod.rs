pub mod crossfade;
pub mod gate;
pub mod lifecycle;
pub mod pixmap;
pub mod wash;

use std::{path::PathBuf, sync::Arc};

use image::RgbaImage;
use kernel::domain::geometry::Cells;
use ratatui::layout::Rect;

#[derive(Debug, Clone)]
pub struct CoverImage {
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
    Running { progress: f32, screen_width: Cells },
    Idle,
}

#[derive(Debug, Clone, Copy)]
pub struct CoverRefresh {
    pub cover: Option<Rect>,
    pub crossfade: CrossfadePermit,
    pub wash: CoverWash,
}
