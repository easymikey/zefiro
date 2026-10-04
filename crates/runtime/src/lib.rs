#![forbid(unsafe_code)]

mod driver;
mod driver_wait;
mod error;
mod event_loop;
mod host;
mod interpret;
mod jobs;
mod latest;
mod macos;
mod outbox;
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

pub use error::Error;
pub use event_loop::run;
pub use host::run_on_main_thread;
pub use latest::{LatestReceiver, LatestReceivers, LatestSenders, latest_channels};
pub use repaint::FRAME_INTERVAL;
pub use runtime::{Runtime, StartupPaths};
pub use shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect};
pub use spawn::Spawners;
