#![forbid(unsafe_code)]

mod error;
mod shell;
mod startup;
mod termination;

use std::{io::Write, process::ExitCode, thread};

use crossbeam_channel::bounded;
use error::Error;
use runtime::{Runtime, Spawners};
use shell::{Painter, ShellEvent};
use startup::{Boot, Look};
use terminal::{TerminalEnvironment, TerminalSession, run_input};

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
    let Boot {
        startup,
        paths,
        look,
    } = startup::boot()?;
    let runtime = Runtime::start(startup, &paths, &Spawners::hardware())?;
    runtime::run_on_main_thread(runtime, move |runtime| run_shell(runtime, look))
        .map_err(Error::from)?
}

fn run_shell(runtime: Runtime, look: Look) -> Result<(), Error> {
    let mut session = TerminalSession::enter()?;
    let brand = terminal::Brand::detect(&TerminalEnvironment::current());
    let probe_answer = terminal::probe(brand);

    let (input_sender, input_receiver) = bounded(256);
    termination::install(input_sender.clone())?;
    let mut painter = Painter::new(session.terminal_mut(), look, probe_answer);
    thread::spawn(move || run_input(ShellEvent::Terminal, &input_sender));

    let outcome = runtime::run(runtime, &mut painter, &input_receiver);
    let teardown = session.restore();
    with_worker_panic(merge_exit_errors(outcome, teardown))
}

fn with_worker_panic(result: Result<(), Error>) -> Result<(), Error> {
    match (result, termination::take_worker_panic()) {
        (Ok(()), Some(report)) => Err(Error::WorkerPanic { report }),
        (result, _) => result,
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
