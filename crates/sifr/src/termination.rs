use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwapOption;
use crossbeam_channel::Sender;

use crate::{error::Error, shell::ShellEvent};

static TERMINATE_SENDER: OnceLock<Sender<ShellEvent>> = OnceLock::new();

static WORKER_PANIC: ArcSwapOption<String> = ArcSwapOption::const_empty();

pub(crate) fn install(sender: Sender<ShellEvent>) -> Result<(), Error> {
    TERMINATE_SENDER
        .set(sender)
        .map_err(|_| Error::SignalHandlerInstalled)?;
    spawn_signal_thread();
    Ok(())
}

#[cfg(unix)]
fn spawn_signal_thread() {
    use std::thread;

    use signal_hook::{
        consts::{SIGHUP, SIGINT, SIGTERM},
        iterator::Signals,
    };

    if let Ok(mut signals) = Signals::new([SIGTERM, SIGHUP, SIGINT]) {
        thread::spawn(move || {
            if signals.forever().next().is_some() {
                terminate_if_listening();
            }
        });
    }
}

#[cfg(not(unix))]
fn spawn_signal_thread() {}

fn terminate_if_listening() {
    if let Some(sender) = TERMINATE_SENDER.get() {
        sender.send(ShellEvent::Terminate).ok();
    }
}

pub(crate) fn remember_worker_panic(report: String) {
    WORKER_PANIC.compare_and_swap(&None::<Arc<String>>, Some(Arc::new(report)));
    terminate_if_listening();
}

pub(crate) fn take_worker_panic() -> Option<String> {
    WORKER_PANIC.swap(None).map(Arc::unwrap_or_clone)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use crate::{
        error::Error,
        termination::{install, remember_worker_panic, take_worker_panic},
    };

    #[test]
    fn the_first_worker_panic_is_the_one_reported_and_it_is_taken_once() {
        remember_worker_panic("the input thread fell over".to_owned());
        remember_worker_panic("a later thread fell over too".to_owned());

        assert_eq!(
            take_worker_panic().as_deref(),
            Some("the input thread fell over")
        );
        assert_eq!(take_worker_panic(), None);
    }

    #[test]
    fn a_second_install_is_refused() {
        let (first, _first_receiver) = bounded(1);
        let (second, _second_receiver) = bounded(1);

        assert!(install(first).is_ok());
        assert!(matches!(
            install(second),
            Err(Error::SignalHandlerInstalled)
        ));
    }
}
