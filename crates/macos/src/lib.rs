#![cfg(target_os = "macos")]

mod audio_hardware;
mod clock;
mod controls;
mod cover;
mod echo;
mod ffi;
mod now_playing;
mod output;
mod system_loop;
mod volume;

pub use objc2::MainThreadMarker;
use objc2_core_foundation::{CFRunLoop, CFRunLoopRunResult, CFString, CFTimeInterval};

pub use crate::{controls::Controls, cover::CoverReader, system_loop::SystemLoop};

const PUMP_SECONDS: CFTimeInterval = 0.0;

pub fn pump_main_run_loop(_main_thread: MainThreadMarker) {
    let mode = CFString::from_static_str("kCFRunLoopDefaultMode");
    while CFRunLoop::run_in_mode(Some(&mode), PUMP_SECONDS, true)
        == CFRunLoopRunResult::HandledSource
    {}
}
