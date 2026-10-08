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
        changed: fn(Result<(), IoError>) -> M,
    },
    Unwatch(PathBuf),
}

pub(crate) fn replace<T: PartialEq>(field: &mut T, next: T) -> Result<(), Unhandled> {
    if *field == next {
        Err(Unhandled)
    } else {
        *field = next;
        Ok(())
    }
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
        update::machine::{Unhandled, each_handled},
    };

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
