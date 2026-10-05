use std::{path::PathBuf, time::Duration};

use crate::{cmd::Cmd, domain::io_error::IoError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unhandled;

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a state machine — it has no `Machine` impl",
    label = "missing `impl Machine for {Self}`",
    note = "every (state, message) pair is a row of this machine's transition table"
)]
pub trait Machine {
    type Message;
    type Effect;

    fn transition(&mut self, message: Self::Message)
    -> Result<Self::Effect, Unhandled>;
}

pub trait Driver: Machine {
    type Effect;

    fn execute(&mut self, effect: <Self as Driver>::Effect) -> Option<Self::Message>;
}

#[derive(Debug)]
pub enum LoopEffect<E, J, M> {
    Execute(E),
    Run(J),
    After {
        delay: Duration,
        message: M,
    },
    Watch {
        path: PathBuf,
        item: fn(Result<(), IoError>) -> M,
    },
    Unwatch(PathBuf),
}

pub type LoopCmd<E, J, M, V> = Cmd<LoopEffect<E, J, M>, V>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::update::machine::{Machine, Unhandled};

    #[derive(Debug, PartialEq)]
    enum Latch {
        Open,
        Closed,
    }

    #[derive(Debug)]
    enum LatchMessage {
        Close,
        Open,
    }

    #[derive(Debug, PartialEq)]
    enum Click {
        Clicked,
    }

    impl Machine for Latch {
        type Message = LatchMessage;
        type Effect = Click;

        fn transition(&mut self, message: LatchMessage) -> Result<Click, Unhandled> {
            match (&*self, message) {
                (Latch::Open, LatchMessage::Close) => {
                    *self = Latch::Closed;
                    Ok(Click::Clicked)
                }
                (Latch::Closed, LatchMessage::Open) => {
                    *self = Latch::Open;
                    Ok(Click::Clicked)
                }
                (Latch::Open, LatchMessage::Open)
                | (Latch::Closed, LatchMessage::Close) => Err(Unhandled),
            }
        }
    }

    struct LatchRow {
        start: Latch,
        message: LatchMessage,
        next: Latch,
        result: Result<Click, Unhandled>,
    }

    #[rstest]
    #[case::open_closes(LatchRow {
        start: Latch::Open,
        message: LatchMessage::Close,
        next: Latch::Closed,
        result: Ok(Click::Clicked),
    })]
    #[case::closed_opens(LatchRow {
        start: Latch::Closed,
        message: LatchMessage::Open,
        next: Latch::Open,
        result: Ok(Click::Clicked),
    })]
    #[case::open_refuses_open(LatchRow {
        start: Latch::Open,
        message: LatchMessage::Open,
        next: Latch::Open,
        result: Err(Unhandled),
    })]
    #[case::closed_refuses_close(LatchRow {
        start: Latch::Closed,
        message: LatchMessage::Close,
        next: Latch::Closed,
        result: Err(Unhandled),
    })]
    fn transition_writes_only_on_success(#[case] row: LatchRow) {
        let mut slot = row.start;
        let result = slot.transition(row.message);
        assert_eq!(slot, row.next);
        assert_eq!(result, row.result);
    }
}
