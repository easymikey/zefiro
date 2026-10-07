pub mod crossfade;
pub mod lifecycle;
pub mod pixmap;
pub mod plan;

use std::{path::PathBuf, sync::Arc};

use image::RgbaImage;

#[derive(Debug, Clone)]
pub struct CoverImage {
    pub path: PathBuf,
    pub image: Arc<RgbaImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverMotion {
    Moving,
    Still,
}
