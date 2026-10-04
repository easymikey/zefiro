#![cfg(target_os = "macos")]

mod clock;
pub(crate) mod controls;
pub(crate) mod core_audio;
pub mod cover;
pub mod driver;
mod ffi;
pub(crate) mod hardware;
pub mod main_loop;
mod now_playing;
