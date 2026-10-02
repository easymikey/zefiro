#![forbid(unsafe_code)]

mod audio;
mod config;
mod driver;
mod error;
mod event_loop;
mod host;
mod interpret;
mod latest;
mod library;
mod macos;
mod paint;
mod port;
mod registry;
mod repaint;
mod runtime;
mod shell;
mod spawn;
mod timers;
mod trace;
mod watcher;
mod wiring;

pub use config::{ConfigPaths, SeenTexts};
pub use error::Error;
pub use event_loop::run;
pub use host::run_on_main_thread;
pub use latest::{LatestReceiver, LatestReceivers, LatestSenders, latest_channels};
pub use library::cover::{CoverDecoded, CoverError, CoverOutcome, CoverRequest};
pub use repaint::FRAME_INTERVAL;
pub use runtime::{Runtime, StartupPaths};
pub use shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect};
pub use spawn::Spawners;
