use crossbeam_channel::Sender;
use dispatch2::DispatchQueue;
use kernel::Message;
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRunLoop;

use crate::controls::Controls;

#[derive(Debug)]
pub struct MainLoop {
    _controls: Controls,
}

impl MainLoop {
    #[must_use]
    pub fn attach(mailbox: &Sender<Message>) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        Some(Self {
            _controls: Controls::attach(main_thread, mailbox),
        })
    }

    #[must_use]
    pub fn stopper(&self) -> LoopStopper {
        LoopStopper
    }

    pub fn run(self) {
        CFRunLoop::run();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoopStopper;

impl LoopStopper {
    pub fn stop(self) {
        DispatchQueue::main().exec_async(|| {
            if let Some(current) = CFRunLoop::current() {
                current.stop();
            }
        });
    }
}
