use std::{path::PathBuf, time::Duration};

use crate::{
    cmd::Cmd,
    domain::{cursor::Cursor, io_error::IoError},
};

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
        changed: fn(Result<(), IoError>) -> M,
    },
    Unwatch(PathBuf),
}

pub(crate) fn move_cursor(
    cursor: &mut Cursor,
    moved_cursor: Cursor,
) -> Result<Cmd, Unhandled> {
    if moved_cursor == *cursor {
        return Err(Unhandled);
    }
    *cursor = moved_cursor;
    Ok(Cmd::none())
}

pub type LoopCmd<E, J, M, V> = Cmd<LoopEffect<E, J, M>, V>;

pub fn each_handled<T, E, M>(
    asked: Vec<T>,
    mut run: impl FnMut(T) -> Result<Cmd<E, M>, Unhandled>,
) -> Result<Cmd<E, M>, Unhandled> {
    asked
        .into_iter()
        .map(&mut run)
        .reduce(|joined, each| match (joined, each) {
            (Ok(first), Ok(second)) => Ok(first.then(second)),
            (Ok(handled), Err(Unhandled)) | (Err(Unhandled), Ok(handled)) => {
                Ok(handled)
            }
            (Err(Unhandled), Err(Unhandled)) => Err(Unhandled),
        })
        .unwrap_or(Err(Unhandled))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        cmd::Cmd,
        update::machine::{Machine, Unhandled, each_handled},
    };

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

        fn transition(
            &mut self,
            latch_message: LatchMessage,
        ) -> Result<Click, Unhandled> {
            match (&*self, latch_message) {
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
        latch: Latch,
        message: LatchMessage,
        next: Latch,
        result: Result<Click, Unhandled>,
    }

    #[rstest]
    #[case::open_closes(LatchRow {
        latch: Latch::Open,
        message: LatchMessage::Close,
        next: Latch::Closed,
        result: Ok(Click::Clicked),
    })]
    #[case::closed_opens(LatchRow {
        latch: Latch::Closed,
        message: LatchMessage::Open,
        next: Latch::Open,
        result: Ok(Click::Clicked),
    })]
    #[case::open_refuses_open(LatchRow {
        latch: Latch::Open,
        message: LatchMessage::Open,
        next: Latch::Open,
        result: Err(Unhandled),
    })]
    #[case::closed_refuses_close(LatchRow {
        latch: Latch::Closed,
        message: LatchMessage::Close,
        next: Latch::Closed,
        result: Err(Unhandled),
    })]
    fn transition_writes_only_on_success(#[case] row: LatchRow) {
        let mut slot = row.latch;
        let result = slot.transition(row.message);
        assert_eq!(slot, row.next);
        assert_eq!(result, row.result);
    }

    struct EachRow {
        asked: Vec<u32>,
        accepted: Vec<u32>,
        result: Result<Cmd<u32, u32>, Unhandled>,
    }

    #[rstest]
    #[case::all_rejected_is_unhandled(EachRow {
        asked: vec![1, 2],
        accepted: Vec::new(),
        result: Err(Unhandled),
    })]
    #[case::one_accepted_keeps_its_cmd(EachRow {
        asked: vec![1, 2],
        accepted: vec![2],
        result: Ok(Cmd::effect(2)),
    })]
    #[case::all_accepted_keep_their_order(EachRow {
        asked: vec![1, 2],
        accepted: vec![1, 2],
        result: Ok(Cmd::effect(1).then(Cmd::effect(2))),
    })]
    #[case::an_empty_batch_is_unhandled(EachRow {
        asked: Vec::new(),
        accepted: vec![1],
        result: Err(Unhandled),
    })]
    fn each_handled_fails_only_when_every_item_is_rejected(#[case] row: EachRow) {
        let result = each_handled(row.asked, |each| {
            if row.accepted.contains(&each) {
                Ok(Cmd::effect(each))
            } else {
                Err(Unhandled)
            }
        });
        assert_eq!(result, row.result);
    }
}
