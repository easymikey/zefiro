#![forbid(unsafe_code)]

mod error;
mod shell;
mod signal;
mod startup;
mod toast;

use std::{io::Write, process::ExitCode, thread};

use crossbeam_channel::unbounded;
use error::Error;
use runtime::{Hardware, Runtime};
use shell::ShellInput;
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
            report(&error);
            ExitCode::FAILURE
        }
    }
}

fn report(error: &Error) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "sifr: {error}");
}

fn run() -> Result<(), Error> {
    terminal::install_panic_hook(signal::remember_worker_panic);
    let (startup, paths) = startup::boot()?;
    let hardware = Hardware::system(&startup);
    let runtime = Runtime::boot(startup, paths, hardware)?;
    let mut session = TerminalSession::enter()?;
    let upgrade = capability_upgrade();

    let (input_sender, input_receiver) = unbounded();
    signal::install(input_sender.clone());
    let mut shell = shell::Shell::new(session.terminal_mut(), input_sender.clone());
    if let Some(answer) = upgrade {
        shell.adopt(answer);
    }
    spawn_terminal_input(input_sender);

    let outcome = runtime::run(runtime, &mut shell, &input_receiver);
    let teardown = session.restore();
    with_worker_panic(combine(outcome, teardown))
}

fn capability_upgrade() -> Option<ProbeAnswer> {
    let brand = terminal::detect(&TerminalEnvironment::current());
    CapabilityProbe::new(brand).and_then(CapabilityProbe::run)
}

fn spawn_terminal_input(shell_input: crossbeam_channel::Sender<ShellInput>) {
    let (terminal_sender, terminal_receiver) = unbounded();
    thread::spawn(move || InputLoop.run(&terminal_sender));
    thread::spawn(move || {
        for event in terminal_receiver.iter() {
            if shell_input.send(ShellInput::Terminal(event)).is_err() {
                return;
            }
        }
    });
}

fn with_worker_panic(result: Result<(), Error>) -> Result<(), Error> {
    match (result, signal::taken_worker_panic()) {
        (Ok(()), Some(report)) => Err(Error::WorkerPanic { report }),
        (result, _) => result,
    }
}

fn combine(
    outcome: Result<(), runtime::RunError<std::io::Error>>,
    teardown: Result<(), std::io::Error>,
) -> Result<(), Error> {
    match (outcome, teardown) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(teardown_error)) => {
            Err(terminal::TerminalError::Teardown(teardown_error).into())
        }
        (Err(run_error), Ok(())) => Err(run_error.into()),
        (Err(run), Err(teardown)) => Err(Error::RunAndTeardown { run, teardown }),
    }
}
