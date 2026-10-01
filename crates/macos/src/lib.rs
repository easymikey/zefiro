#![cfg(target_os = "macos")]

mod clock;
mod controls;
mod core_audio;
mod cover;
mod ffi;
mod hardware_state;
mod macos_loop;
mod main_loop;
mod now_playing;

pub use crate::{
    cover::CoverReader,
    macos_loop::MacosLoop,
    main_loop::{LoopStopper, MainLoop},
};
