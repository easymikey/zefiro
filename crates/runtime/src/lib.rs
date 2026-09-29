#![forbid(unsafe_code)]

mod audio;
mod cells;
mod config;
mod driver;
mod error;
mod event_loop;
mod host;
mod interpret;
mod launch;
mod library;
mod macos;
mod mailbox;
mod paint;
mod port;
mod registry;
mod repaint;
mod runtime;
mod shell;
mod timers;
mod trace;
mod wiring;

pub use cells::{Cells, Reading, Writers, cells};
pub use config::{ConfigPaths, SeenTexts};
pub use error::{HostError, RunError, RuntimeError};
pub use event_loop::run;
pub use host::host;
pub use launch::Launchers;
pub use library::cover::{CoverDecoded, CoverError, CoverOutcome, CoverRequest};
pub use repaint::FRAME;
pub use runtime::{BootPaths, Runtime};
pub use shell::{FrameDue, Painted, Reaction, Shell, ShellEffect, View};
