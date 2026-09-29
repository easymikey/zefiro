use std::thread;

use crate::{error::HostError, runtime::Runtime};

#[cfg(target_os = "macos")]
pub fn host<R, F>(runtime: Runtime, body: F) -> Result<R, HostError>
where
    R: Send + 'static,
    F: FnOnce(Runtime) -> R + Send + 'static,
{
    debug_assert!(matches!(
        crate::registry::row(kernel::domain::Driver::Macos).placement,
        crate::registry::Placement::WorkerWithMainLoop
    ));
    let Some(main) = ::macos::MainLoop::attach(&runtime.mailbox_sender()) else {
        let mut runtime = runtime;
        runtime
            .trace
            .push(crate::trace::TraceEntry::ControlsUnattached);
        return Ok(body(runtime));
    };
    let guard = StopOnDrop(main.stopper());
    let handle = thread::Builder::new()
        .name("sifr-event-loop".to_owned())
        .spawn(move || {
            let _guard = guard;
            body(runtime)
        })
        .map_err(HostError::Spawn)?;
    main.run();
    handle.join().map_err(|_| HostError::EventLoopPanicked)
}

#[cfg(not(target_os = "macos"))]
pub fn host<R, F>(runtime: Runtime, body: F) -> Result<R, HostError>
where
    R: Send + 'static,
    F: FnOnce(Runtime) -> R + Send + 'static,
{
    Ok(body(runtime))
}

#[cfg(target_os = "macos")]
struct StopOnDrop(::macos::LoopStopper);

#[cfg(target_os = "macos")]
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::Startup;

    use crate::{host::host, runtime::Runtime, trace::Trace, wiring::Wiring};

    fn idle_runtime() -> Runtime {
        let (wiring, ..) = Wiring::idle();
        let seed = Runtime::seeded(Startup::default());
        Runtime::assemble(seed, wiring, Trace::default())
    }

    #[test]
    fn off_the_main_thread_the_body_runs_inline() {
        let runtime = idle_runtime();

        let outcome = host(runtime, |runtime| {
            #[cfg(target_os = "macos")]
            assert!(runtime.trace().iter().any(|entry| matches!(
                entry,
                crate::trace::TraceEntry::ControlsUnattached
            )));
            runtime.drain();
            42
        });

        assert_eq!(outcome.unwrap(), 42);
    }
}
