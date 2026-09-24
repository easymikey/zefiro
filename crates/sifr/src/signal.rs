use std::sync::{Mutex, OnceLock};

use crossbeam_channel::Sender;

use crate::shell::ShellInput;

static QUIT_SENDER: OnceLock<Sender<ShellInput>> = OnceLock::new();

static WORKER_PANIC: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn install(sender: Sender<ShellInput>) {
    let _ = QUIT_SENDER.set(sender);
    spawn_watcher();
}

#[cfg(unix)]
fn spawn_watcher() {
    use std::thread;

    use signal_hook::{
        consts::{SIGHUP, SIGINT, SIGTERM},
        iterator::Signals,
    };

    if let Ok(mut signals) = Signals::new([SIGTERM, SIGHUP, SIGINT]) {
        thread::spawn(move || {
            if signals.forever().next().is_some() {
                send_terminate();
            }
        });
    }
}

#[cfg(not(unix))]
fn spawn_watcher() {}

fn send_terminate() {
    if let Some(sender) = QUIT_SENDER.get() {
        let _ = sender.send(ShellInput::Terminate);
    }
}

pub(crate) fn remember_worker_panic(report: String) {
    if let Ok(mut slot) = WORKER_PANIC.lock() {
        let _ = slot.get_or_insert(report);
    }
    send_terminate();
}

pub(crate) fn taken_worker_panic() -> Option<String> {
    WORKER_PANIC.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use crate::signal::{remember_worker_panic, taken_worker_panic};

    #[test]
    fn the_first_worker_panic_is_the_one_reported_and_it_is_taken_once() {
        remember_worker_panic("the input thread fell over".to_owned());
        remember_worker_panic("a later thread fell over too".to_owned());

        assert_eq!(
            taken_worker_panic().as_deref(),
            Some("the input thread fell over")
        );
        assert_eq!(taken_worker_panic(), None);
    }
}
