#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

#[cfg(test)] extern crate self as widgets;

mod animation;
mod braille;
mod card;
mod geometry;
mod key_hints;
mod milkdrop;
mod overlay;
mod pixels;
mod playlist;
mod primitive;
mod repaint;
mod scene;
mod screen;
mod spectrum;
mod status_line;
#[cfg(test)] mod test_support;
mod theme;
mod toast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playing {
    Yes,
    No,
}

pub use animation::{
    AnimationStage,
    AnimationTimings,
    Backdrop,
    VolumeShades,
    chip_pulse,
    delete_burst,
    favorite_pulse,
    modal_in,
    modal_out,
    row_flash,
    screen_wash,
    toast_burst,
    toast_slide_in,
    volume_pulse,
    wash_reveal,
};
pub use card::{CardMetrics, CoverArt};
pub use geometry::DEFAULT_CELL_ASPECT;
pub use milkdrop::{MilkdropAdvance, MilkdropColors, MilkdropField, lines_into};
pub use overlay::modal::{ModalAreas, ModalScrollAreas, OverlayAreas};
pub use pixels::{
    CellPixels,
    CoverCrossfade,
    CoverFrame,
    CoverLifecycle,
    CoverMotion,
    CoverRefreshParts,
    CoverUpdate,
    CoverWash,
    CrossfadePermit,
    CrossfadeStage,
    DecodedCover,
    MilkdropCover,
    PixmapSource,
    VinylCache,
    VinylCacheKey,
    VinylStyle,
    blend_by_column,
    channel_byte,
    column_reveal,
};
pub use playlist::{PlaylistAreas, favorite_cell};
pub use repaint::{
    OnScreen,
    Presence,
    ProgressScale,
    next_clock_second,
    next_progress_step,
    next_sleep_minute,
};
pub use scene::{PixelPath, Scene, abbreviate_home};
pub use screen::{Breakpoint, FrameLayout, FrameLayoutParts, Screen};
pub use spectrum::{SPECTRUM_BANDS, Spectrum, SpectrumMotion, SpectrumSmoothing};
pub use theme::{
    ActiveTheme,
    ColorDepth,
    Colors,
    Role,
    Theme,
    color_at_depth,
    lerp_rgb,
    shade,
};
pub use toast::ToastAreas;
