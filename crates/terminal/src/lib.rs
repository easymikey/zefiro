#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod caps;
mod error;
mod input;
mod keys;
mod pixels;
mod session;
mod window_colors;

pub use caps::{
    Brand,
    Capabilities,
    CapabilityProbe,
    ProbeAnswer,
    TerminalEnvironment,
    cell_aspect,
    detect,
    resolve_immediate,
};
pub use error::TerminalError;
pub use input::InputLoop;
pub use keys::{LayoutTranslation, from_event};
pub use pixels::{
    CoverArtOwner,
    CoverFade,
    CoverKey,
    CoverLook,
    CoverMoment,
    CoverMotion,
    CoverPlacement,
    CoverSources,
    CoverWash,
    DecodedCover,
    Pixels,
};
pub use session::{TerminalSession, install_panic_hook};
pub use window_colors::{
    UnknownThemeError,
    window_colors_sequence,
    write_window_colors,
};
