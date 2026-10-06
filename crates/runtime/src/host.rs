use crate::{error::Error, runtime::Runtime};

#[cfg(target_os = "macos")]
pub fn run_on_main_thread<R, F>(runtime: Runtime, body: F) -> Result<R, Error>
where
    R: Send + 'static,
    F: FnOnce(Runtime) -> R + Send + 'static,
{
    let Some(main) = ::macos::main_loop::MainLoop::attach(
        &runtime.wiring.macos_channel.callback_sender,
    ) else {
        return Ok(body(runtime));
    };
    let guard = StopOnDrop(main.stopper());
    let handle = std::thread::Builder::new()
        .name("sifr-event-loop".to_owned())
        .spawn(move || {
            let _guard = guard;
            body(runtime)
        })
        .map_err(Error::Host)?;
    main.run();
    handle.join().map_err(|_| Error::EventLoopPanicked)
}

#[cfg(not(target_os = "macos"))]
pub fn run_on_main_thread<R, F>(runtime: Runtime, body: F) -> Result<R, Error>
where
    R: Send + 'static,
    F: FnOnce(Runtime) -> R + Send + 'static,
{
    Ok(body(runtime))
}

#[cfg(target_os = "macos")]
struct StopOnDrop(::macos::main_loop::MainLoopStop);

#[cfg(target_os = "macos")]
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::startup::Startup;

    use crate::{host::run_on_main_thread, runtime::Runtime, wiring::Wiring};

    fn idle_runtime() -> Runtime {
        let (wiring, ..) = Wiring::idle();
        let started = kernel::update::startup::startup(Startup::default());
        Runtime::assemble(started, wiring).unwrap()
    }

    #[test]
    fn off_the_main_thread_the_body_runs_inline() {
        let runtime = idle_runtime();

        let result = run_on_main_thread(runtime, |runtime| {
            runtime.drain();
            42
        });

        assert_eq!(result.unwrap(), 42);
    }
}
