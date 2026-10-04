#![cfg(target_os = "macos")]

mod clock;
mod controls;
mod core_audio;
mod cover;
mod driver;
mod ffi;
mod hardware;
mod main_loop;
mod now_playing;

pub use crate::{
    controls::{RemoteInput, RemoteInputError},
    core_audio::Error,
    cover::{CoverBytes, CoverReader, MacosJob},
    driver::{MacosDriver, MacosEffect, MacosMessage},
    hardware::HardwarePoll,
    main_loop::{MainLoop, MainLoopStop},
};
