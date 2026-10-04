use crossbeam_channel::Sender;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRunLoop;

use crate::{controls::Controls, message::MacosMessage};

#[derive(Debug)]
pub struct MainLoop {
    _controls: Controls,
}

impl MainLoop {
    #[must_use]
    pub fn attach(heard: &Sender<MacosMessage>) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        Some(Self {
            _controls: Controls::attach(main_thread, heard),
        })
    }

    #[must_use]
    pub fn stopper(&self) -> MainLoopStop {
        MainLoopStop
    }

    pub fn run(self) {
        CFRunLoop::run();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MainLoopStop;

impl MainLoopStop {
    pub fn stop(self) {
        DispatchQueue::main().exec_async(|| {
            if let Some(current) = CFRunLoop::current() {
                current.stop();
            }
        });
    }
}
