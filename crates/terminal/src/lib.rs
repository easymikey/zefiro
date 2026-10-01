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
    ProbeAnswer,
    TerminalEnvironment,
    cell_aspect,
    probe,
};
pub use error::Error;
pub use input::run_input;
pub use keys::{LayoutTranslation, from_event};
pub use pixels::{
    CoverMotion,
    CoverRefreshParts,
    CoverRenderer,
    CoverWash,
    CrossfadePermit,
    DecodedCover,
};
pub use session::{TerminalSession, install_panic_hook};
pub use window_colors::{UnknownThemeError, write_window_colors};
