use std::{
    io,
    sync::{
        Arc,
        Mutex,
        OnceLock,
        PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};

use crate::{error::Error, shell::shell_input::ShellInput};

static TERMINATE_SENDER: OnceLock<Sender<ShellInput>> = OnceLock::new();

static WORKER_PANICKED: AtomicBool = AtomicBool::new(false);

static INPUT_ERROR: Mutex<Option<io::Error>> = Mutex::new(None);

#[cfg(unix)]
const TERMINATING_SIGNALS: [std::ffi::c_int; 3] = [
    signal_hook::consts::SIGTERM,
    signal_hook::consts::SIGHUP,
    signal_hook::consts::SIGINT,
];

pub(crate) enum ThreadStop {
    #[cfg(unix)]
    Signals(signal_hook::iterator::Handle),
    #[cfg(not(unix))]
    Flag(Arc<AtomicBool>),
}

impl ThreadStop {
    fn raise(&self) {
        match self {
            #[cfg(unix)]
            Self::Signals(handle) => handle.close(),
            #[cfg(not(unix))]
            Self::Flag(flag) => flag.store(true, Ordering::Release),
        }
    }
}

pub(crate) struct JoinOnDrop {
    pub(crate) stop: ThreadStop,
    pub(crate) thread: Option<JoinHandle<()>>,
}

impl Drop for JoinOnDrop {
    fn drop(&mut self) {
        self.stop.raise();
        if let Some(Err(_panic)) = self.thread.take().map(JoinHandle::join) {
            WORKER_PANICKED.store(true, Ordering::Release);
        }
    }
}

#[cfg(unix)]
pub(crate) fn install(sender: Sender<ShellInput>) -> Result<JoinOnDrop, Error> {
    install_with(sender, terminating_signals()?)
}

#[cfg(not(unix))]
pub(crate) fn install(sender: Sender<ShellInput>) -> Result<JoinOnDrop, Error> {
    TERMINATE_SENDER
        .set(sender)
        .map_err(|_refused| Error::SignalHandlerInstalled)?;
    Ok(JoinOnDrop {
        stop: ThreadStop::Flag(Arc::default()),
        thread: None,
    })
}

#[cfg(unix)]
fn terminating_signals() -> Result<signal_hook::iterator::Signals, Error> {
    use signal_hook::flag;

    let signalled = Arc::new(AtomicBool::new(false));
    for signal in TERMINATING_SIGNALS {
        flag::register_conditional_default(signal, Arc::clone(&signalled))
            .map_err(Error::SignalHandlers)?;
        flag::register(signal, Arc::clone(&signalled))
            .map_err(Error::SignalHandlers)?;
    }
    signal_hook::iterator::Signals::new(TERMINATING_SIGNALS)
        .map_err(Error::SignalHandlers)
}

#[cfg(unix)]
fn install_with(
    sender: Sender<ShellInput>,
    mut signals: signal_hook::iterator::Signals,
) -> Result<JoinOnDrop, Error> {
    TERMINATE_SENDER
        .set(sender)
        .map_err(|_refused| Error::SignalHandlerInstalled)?;
    let handle = signals.handle();
    let thread = thread::spawn(move || forward(signals.forever()));
    Ok(JoinOnDrop {
        stop: ThreadStop::Signals(handle),
        thread: Some(thread),
    })
}

#[cfg(unix)]
fn forward(signals: impl IntoIterator<Item = std::ffi::c_int>) {
    if signals.into_iter().next().is_some() {
        terminate_if_listening();
    }
}

fn terminate_if_listening() {
    if let Some(sender) = TERMINATE_SENDER.get() {
        match sender.try_send(ShellInput::Terminate) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }
}

pub(crate) fn remember_input_error(error: io::Error) {
    let mut stored = INPUT_ERROR.lock().unwrap_or_else(PoisonError::into_inner);
    *stored = stored.take().or(Some(error));
    drop(stored);
    terminate_if_listening();
}

pub(crate) fn take_input_error() -> Option<io::Error> {
    INPUT_ERROR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

pub(crate) fn remember_worker_panic() {
    WORKER_PANICKED.store(true, Ordering::Release);
    terminate_if_listening();
}

pub(crate) fn take_worker_panic() -> bool {
    WORKER_PANICKED.swap(false, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        panic,
        sync::{Mutex, PoisonError},
        thread,
    };

    use crossbeam_channel::bounded;

    use crate::{
        error::Error,
        shell::shell_input::ShellInput,
        termination::{
            forward,
            install_with,
            remember_input_error,
            remember_worker_panic,
            take_input_error,
            take_worker_panic,
        },
    };

    fn no_signals() -> signal_hook::iterator::Signals {
        signal_hook::iterator::Signals::new(std::iter::empty::<std::ffi::c_int>())
            .unwrap()
    }

    #[test]
    fn a_worker_panic_is_reported_once() {
        remember_worker_panic();
        remember_worker_panic();

        assert!(take_worker_panic());
        assert!(!take_worker_panic());
    }

    #[test]
    fn the_first_input_error_is_reported_once() {
        remember_input_error(io::Error::other("first"));
        remember_input_error(io::Error::other("second"));

        assert_eq!(
            take_input_error().map(|error| error.to_string()),
            Some("first".to_owned())
        );
        assert!(take_input_error().is_none());
    }

    #[test]
    fn a_signal_terminates_and_a_second_install_is_refused() {
        let (first, first_receiver) = bounded(1);
        let (second, _second_receiver) = bounded(1);

        let watch = install_with(first, no_signals()).unwrap();
        forward([signal_hook::consts::SIGINT]);

        assert!(matches!(
            first_receiver.try_recv(),
            Ok(ShellInput::Terminate)
        ));
        assert!(matches!(
            install_with(second, no_signals()),
            Err(Error::SignalHandlerInstalled)
        ));
        drop(watch);
    }

    static ORIGINAL_HOOK_THREADS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn record_thread(_info: &panic::PanicHookInfo<'_>) {
        let name = thread::current().name().map(str::to_owned);
        ORIGINAL_HOOK_THREADS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(name);
    }

    #[test]
    fn with_the_hook_installed_a_driver_or_paint_panic_chains_the_original_hook() {
        let painting = thread::current().name().map(str::to_owned);
        panic::set_hook(Box::new(record_thread));
        terminal::session::install_panic_hook();

        let driver = thread::Builder::new()
            .name("supervised-driver".to_owned())
            .spawn(|| panic::catch_unwind(|| panic!("driver fell over")).is_err())
            .unwrap();
        let driver_caught = driver.join().unwrap();
        let paint_caught = panic::catch_unwind(|| panic!("paint fell over")).is_err();
        drop(panic::take_hook());

        assert!(driver_caught && paint_caught);
        let expected: Vec<String> = ["supervised-driver".to_owned()]
            .into_iter()
            .chain(painting)
            .collect();
        assert_eq!(
            *ORIGINAL_HOOK_THREADS
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            expected
        );
    }
}
