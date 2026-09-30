#![cfg(target_os = "macos")]

mod audio_hardware;
mod clock;
mod controls;
mod cover;
mod cover_slot;
mod echo;
mod ffi;
mod macos_loop;
mod main_loop;
mod now_playing;
mod output;
mod volume;

pub use crate::{
    controls::Controls,
    cover::CoverReader,
    macos_loop::MacosLoop,
    main_loop::{LoopStopper, MainLoop},
};
