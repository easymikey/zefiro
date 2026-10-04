#![forbid(unsafe_code)]

mod error;
mod shell;
mod startup;
mod termination;

use std::{io::Write, process::ExitCode};

use crossbeam_channel::{Sender, bounded};
use error::Error;
use kernel::{Diagnostic, PaintError};
use runtime::{Runtime, Spawners};
use shell::{Painter, ShellInput};
use startup::Launch;
use terminal::{TerminalEnvironment, TerminalSession};

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
    terminal::install_panic_hook(termination::remember_worker_panic);
    let Launch {
        startup,
        paths,
        theme,
    } = startup::launch()?;
    let runtime = Runtime::start(startup, &paths, &Spawners::hardware())?;
    runtime::run_on_main_thread(runtime, move |runtime| run_shell(runtime, theme))
        .map_err(Error::from)?
}

fn run_shell(runtime: Runtime, theme: config::TomlTheme) -> Result<(), Error> {
    let mut session = TerminalSession::enter()?;
    let brand = terminal::TerminalApp::detect(&TerminalEnvironment::current());
    let (input_sender, input_receiver) = bounded(256);
    let probe_answer = probed(brand, &input_sender)?;
    let signal_thread = termination::install(input_sender.clone())?;
    let mut painter =
        Painter::new(session.terminal_mut(), theme, capabilities(probe_answer));
    let input_thread = termination::spawn_input(input_sender);

    let outcome = runtime::run(runtime, &mut painter, &input_receiver);
    drop(input_receiver);
    drop(input_thread);
    let teardown = session.restore();
    drop(signal_thread);
    with_thread_failures(merge_exit_errors(outcome, teardown))
}

fn probed(
    brand: terminal::TerminalApp,
    input_sender: &Sender<ShellInput>,
) -> Result<Option<terminal::ProbeAnswer>, Error> {
    match terminal::probe(brand) {
        Ok(answer) => Ok(answer),
        Err(error) => {
            input_sender
                .send(ShellInput::Error(PaintError::Probe(
                    Diagnostic::from_error(&error),
                )))
                .map_err(|_closed| runtime::Error::InputClosed)?;
            Ok(None)
        }
    }
}

fn capabilities(probe_answer: Option<terminal::ProbeAnswer>) -> terminal::Capabilities {
    let found =
        terminal::Capabilities::from_environment(&TerminalEnvironment::current());
    match probe_answer {
        Some(answer) => terminal::Capabilities {
            picker: answer.picker,
            pixel_path: widgets::PixelPath::Protocol,
            ..found
        },
        None => found,
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
    outcome: Result<(), runtime::Error>,
    teardown: Result<(), std::io::Error>,
) -> Result<(), Error> {
    match (outcome, teardown) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(teardown_error)) => {
            Err(terminal::Error::Teardown(teardown_error).into())
        }
        (Err(run_error), Ok(())) => Err(run_error.into()),
        (Err(run), Err(teardown)) => Err(Error::RunAndTeardown { run, teardown }),
    }
}
