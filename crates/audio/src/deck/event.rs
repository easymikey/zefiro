use kernel::domain::{revision::Revision, transport::OutputError};

#[derive(Debug)]
pub enum DeckEvent {
    OutputLost(OutputError),
    Woke(Revision),
}
