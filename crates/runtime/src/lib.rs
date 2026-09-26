#![forbid(unsafe_code)]

mod audio;
mod config;
mod driver;
mod error;
mod event_loop;
mod host;
mod interpret;
mod library;
mod macos;
mod mailbox;
mod port;
mod registry;
mod runtime;
mod shell;
mod timers;
mod trace;

pub use config::ConfigPaths;
pub use driver::{DriverLoop, NoDriver};
pub use error::{HostError, RunError, RuntimeError};
pub use event_loop::run;
pub use host::host;
pub use library::cover::{CoverDecoded, CoverError, CoverOutcome, CoverRequest};
#[cfg(target_os = "macos")] pub use macos::SystemStart;
pub use mailbox::Mailbox;
pub use runtime::{BootPaths, Hardware, Runtime};
pub use shell::{FrameDue, Painted, Reaction, Reload, Shell, ShellEffect, View};
pub use trace::{Trace, TraceEntry};
