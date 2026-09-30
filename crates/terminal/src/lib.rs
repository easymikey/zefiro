#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod capabilities;
mod error;
mod input;
mod keys;
mod pixels;
mod session;
mod window_colors;

pub use capabilities::{
    Brand,
    Capabilities,
    CapabilityProbe,
    ProbeAnswer,
    TerminalEnvironment,
    cell_aspect,
};
pub use error::Error;
pub use input::InputLoop;
pub use keys::{LayoutTranslation, from_event};
pub use pixels::{
    CoverFade,
    CoverKey,
    CoverLook,
    CoverMoment,
    CoverMotion,
    CoverParts,
    CoverPlacement,
    CoverRenderer,
    CoverWash,
    DecodedCover,
    OwnedCoverArt,
};
pub use session::{TerminalSession, install_panic_hook};
pub use window_colors::{
    UnknownThemeError,
    window_colors_sequence,
    write_window_colors,
};
