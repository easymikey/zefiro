use crossterm::event::Event;
use kernel::message::PaintError;

#[derive(Debug, Clone)]
pub(crate) enum ShellInput {
    Terminal(Event),
    Terminate,
    Error(PaintError),
}
