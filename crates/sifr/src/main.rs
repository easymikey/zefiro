#![forbid(unsafe_code)]

mod error;
mod shell;
mod startup;
mod termination;

use std::{io::Write, process::ExitCode};

use crossbeam_channel::{Sender, bounded};
use error::Error;
use kernel::{domain::config::Diagnostic, message::PaintError};
use runtime::{runtime::Runtime, spawn::Spawners};
use shell::{painter::Painter, shell_input::ShellInput};
use startup::Launch;
use terminal::{
    capabilities::{Capabilities, TerminalApp, TerminalEnvironment},
    session::TerminalSession,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            drop(writeln!(std::io::stderr(), "sifr: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let Launch {
        startup,
        paths,
        theme,
        appearance,
    } = startup::launch()?;
    let runtime = Runtime::start(startup, &paths, &Spawners::hardware())?;
    runtime::host::run_on_main_thread(runtime, move |runtime| {
        run_shell(runtime, theme, appearance)
    })
    .map_err(Error::from)?
}

fn run_shell(
    runtime: Runtime,
    theme: config::theme_file::TomlTheme,
    appearance: kernel::domain::appearance::Appearance,
) -> Result<(), Error> {
    terminal::session::install_panic_hook();
    let mut session = TerminalSession::enter()?;
    let environment = TerminalEnvironment::current();
    let app = TerminalApp::from_environment(&environment);
    let (input_sender, input_receiver) = bounded(256);
    let environment_capabilities = Capabilities::from_environment(&environment);
    let capabilities = queried(app, environment_capabilities, &input_sender)?;
    let signal_thread = termination::install(input_sender.clone())?;
    let mut painter = Painter::new(session.terminal_mut(), theme, capabilities)
        .with_appearance(appearance);
    shell::input::spawn_input(input_sender);

    let run_result = runtime::event_loop::run(runtime, &mut painter, &input_receiver);
    drop(input_receiver);
    let teardown = session.restore();
    drop(signal_thread);
    with_thread_failures(merge_exit_errors(run_result, teardown))
}

fn queried(
    app: TerminalApp,
    capabilities: Capabilities,
    input_sender: &Sender<ShellInput>,
) -> Result<Capabilities, Error> {
    match terminal::capabilities::query(app) {
        Ok(Some(picker)) => Ok(Capabilities {
            picker,
            color_depth: capabilities.color_depth,
        }),
        Ok(None) => Ok(capabilities),
        Err(error) => {
            input_sender
                .send(ShellInput::Error(PaintError::Query(
                    Diagnostic::from_error(&error),
                )))
                .map_err(|_closed| runtime::error::Error::InputClosed)?;
            Ok(capabilities)
        }
    }
}

fn with_thread_failures(result: Result<(), Error>) -> Result<(), Error> {
    match (
        result,
        termination::take_worker_panic(),
        termination::take_input_error(),
    ) {
        (Ok(()), true, _) => Err(Error::WorkerPanicked),
        (Ok(()), false, Some(error)) => Err(Error::Input(error)),
        (result, _, _) => result,
    }
}

fn merge_exit_errors(
    run_result: Result<(), runtime::error::Error>,
    teardown: Result<(), std::io::Error>,
) -> Result<(), Error> {
    match (run_result, teardown) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(teardown_error)) => {
            Err(terminal::error::Error::Teardown(teardown_error).into())
        }
        (Err(run_error), Ok(())) => Err(run_error.into()),
        (Err(run_error), Err(teardown_error)) => Err(Error::RunAndTeardown {
            run_error,
            teardown_error,
        }),
    }
}
