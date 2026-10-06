#[cfg(target_os = "macos")]
fn main() {
    stop_from_another_thread_ends_run();
    stop_sent_before_run_also_ends_run();
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn stop_from_another_thread_ends_run() {
    use std::{thread, time::Duration};

    let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(1);
    let attached = macos::main_loop::MainLoop::attach(&callback_sender);
    assert!(attached.is_some(), "expected the real main thread");
    if let Some(main_loop) = attached {
        let stopper = main_loop.stopper();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            stopper.stop();
        });
        main_loop.run();
    }
}

#[cfg(target_os = "macos")]
fn stop_sent_before_run_also_ends_run() {
    let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(1);
    let attached = macos::main_loop::MainLoop::attach(&callback_sender);
    assert!(attached.is_some(), "expected the real main thread");
    if let Some(main_loop) = attached {
        main_loop.stopper().stop();
        main_loop.run();
    }
}
