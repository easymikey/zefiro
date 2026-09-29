#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod animation;
mod braille;
mod card;
mod geometry;
mod key_hints;
mod milkdrop;
mod overlay;
mod playlist;
mod primitive;
mod redraw;
mod scene;
mod screen;
mod spectrum;
mod status_line;
mod theme;
mod toast;

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
pub use geometry::{CellAspect, Cells, CoverAspect, CoverSizing, Pixels};
pub use milkdrop::{
    MilkdropAdvance,
    MilkdropColors,
    MilkdropField,
    Playing,
    lines_into,
};
pub use overlay::modal::{ModalAreas, ModalScrollAreas, OverlayAreas};
pub use playlist::{PlaylistAreas, favorite_cell};
pub use redraw::{
    OnScreen,
    Presence,
    ProgressScale,
    next_clock_second,
    next_progress_step,
    next_sleep_minute,
};
pub use scene::{PixelPath, Scene, abbreviate_home};
pub use screen::{Breakpoint, FrameLayout, LayoutInputs, Screen};
pub use spectrum::{SPECTRUM_BANDS, Spectrum, SpectrumMotion, SpectrumSmoothing};
pub use theme::{
    ActiveTheme,
    ColorDepth,
    Colors,
    Role,
    Theme,
    bar_colors,
    color_at_depth,
    detect,
    lerp_rgb,
};
pub use toast::ToastAreas;
