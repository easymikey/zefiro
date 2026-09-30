#![forbid(unsafe_code)]

mod error;
mod shell;
mod signal;
mod startup;
mod toast;

use std::{io::Write, process::ExitCode, thread};

use crossbeam_channel::bounded;
use error::Error;
use runtime::{Runtime, Spawners};
use shell::ShellInput;
use startup::{Look, Prepared};
use terminal::{
    CapabilityProbe,
    InputLoop,
    ProbeAnswer,
    TerminalEnvironment,
    TerminalSession,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn print_error(error: &Error) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "sifr: {error}");
}

fn run() -> Result<(), Error> {
    terminal::install_panic_hook(signal::remember_worker_panic);
    let Prepared {
        startup,
        paths,
        look,
    } = startup::prepare()?;
    let runtime = Runtime::start(startup, &paths, &Spawners::hardware())?;
    runtime::run_on_main_thread(runtime, move |runtime| run_shell(runtime, look))
        .map_err(Error::from)?
}

fn run_shell(runtime: Runtime, look: Look) -> Result<(), Error> {
    let mut session = TerminalSession::enter()?;
    let upgrade = probe_capabilities();

    let (input_sender, input_receiver) = bounded(256);
    signal::apply_reload(input_sender.clone())?;
    let mut shell = shell::Shell::new(session.terminal_mut(), look)?;
    if let Some(answer) = upgrade {
        shell.apply_probe_answer(answer);
    }
    spawn_terminal_input(input_sender);

    let outcome = runtime::run(runtime, &mut shell, &input_receiver);
    let teardown = session.restore();
    with_worker_panic(merge_exit_errors(outcome, teardown))
}

fn probe_capabilities() -> Option<ProbeAnswer> {
    let brand = terminal::Brand::detect(&TerminalEnvironment::current());
    CapabilityProbe::new(brand).and_then(CapabilityProbe::run)
}

fn spawn_terminal_input(shell_input: crossbeam_channel::Sender<ShellInput>) {
    thread::spawn(move || InputLoop.run(ShellInput::Terminal, &shell_input));
}

fn with_worker_panic(result: Result<(), Error>) -> Result<(), Error> {
    match (result, signal::take_worker_panic()) {
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
