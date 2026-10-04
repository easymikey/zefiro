#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod capabilities;
mod error;
mod keys;
mod pixels;
mod session;
mod window_colors;

pub use capabilities::{
    Capabilities,
    ProbeAnswer,
    TerminalApp,
    TerminalEnvironment,
    cell_aspect,
    probe,
};
pub use error::Error;
pub use keys::{LayoutTranslation, from_event};
pub use pixels::CoverPainter;
pub use session::{TerminalSession, install_panic_hook};
pub use window_colors::{UnknownThemeError, write_window_colors};
