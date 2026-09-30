#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod cover;
mod error;
mod memo;
mod numeric;
mod paint;
mod progress;
mod track_identity;
mod vinyl;

pub use cover::cover_aspect_ratio;
pub use error::Error;
pub use numeric::{
    channel_byte,
    dimension_f32,
    dimension_u32,
    floor_u32,
    floor_usize,
    round_u32,
    round_usize,
    unit_fraction,
};
pub use progress::{BarColorOverrides, BarColors, ProgressGeometry, color_overrides};
pub use track_identity::TrackIdentity;
pub use vinyl::{
    ArtCacheState,
    DecodedArt,
    SleeveFace,
    VinylArtSource,
    VinylCache,
    VinylCacheKey,
    VinylColors,
    VinylImage,
    VinylLayout,
    VinylRequest,
    canvas_aspect_ratio,
    compose,
};
