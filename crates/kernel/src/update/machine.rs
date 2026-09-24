#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a state machine — it has no `Machine` impl",
    label = "missing `impl Machine for {Self}`",
    note = "every (state, message) pair is a row of this machine's transition table"
)]
pub trait Machine: Default {
    type Message;
    type Rejection;
    type Effect;

    fn transition(
        self,
        message: Self::Message,
    ) -> Result<(Self, Self::Effect), Rejected<Self>>;

    fn update(
        &mut self,
        message: Self::Message,
    ) -> Result<Self::Effect, Self::Rejection> {
        match std::mem::take(self).transition(message) {
            Ok((state, effect)) => {
                *self = state;
                Ok(effect)
            }
            Err(Rejected { state, reason }) => {
                *self = state;
                Err(reason)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Never {}

pub struct Rejected<S: Machine> {
    pub state: S,
    pub reason: S::Rejection,
}

impl<S: Machine> std::fmt::Debug for Rejected<S>
where
    S: std::fmt::Debug,
    S::Rejection: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rejected")
            .field("state", &self.state)
            .field("reason", &self.reason)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::update::machine::{Machine, Rejected};

    #[derive(Debug, Default, PartialEq)]
    enum Latch {
        #[default]
        Open,
        Closed,
    }

    #[derive(Debug)]
    enum LatchMessage {
        Close,
        Open,
    }

    #[derive(Debug, PartialEq)]
    enum LatchRejection {
        WhileOpen,
        WhileClosed,
    }

    #[derive(Debug, PartialEq)]
    enum Click {
        Clicked,
    }

    impl Machine for Latch {
        type Message = LatchMessage;
        type Rejection = LatchRejection;
        type Effect = Click;

        fn transition(
            self,
            message: LatchMessage,
        ) -> Result<(Self, Click), Rejected<Self>> {
            match (self, message) {
                (Latch::Open, LatchMessage::Close) => {
                    Ok((Latch::Closed, Click::Clicked))
                }
                (Latch::Closed, LatchMessage::Open) => {
                    Ok((Latch::Open, Click::Clicked))
                }
                (Latch::Open, LatchMessage::Open) => Err(Rejected {
                    state: Latch::Open,
                    reason: LatchRejection::WhileOpen,
                }),
                (Latch::Closed, LatchMessage::Close) => Err(Rejected {
                    state: Latch::Closed,
                    reason: LatchRejection::WhileClosed,
                }),
            }
        }
    }

    struct LatchRow {
        start: Latch,
        message: LatchMessage,
        next: Latch,
        outcome: Result<Click, LatchRejection>,
    }

    #[rstest]
    #[case::open_closes(LatchRow {
        start: Latch::Open,
        message: LatchMessage::Close,
        next: Latch::Closed,
        outcome: Ok(Click::Clicked),
    })]
    #[case::closed_opens(LatchRow {
        start: Latch::Closed,
        message: LatchMessage::Open,
        next: Latch::Open,
        outcome: Ok(Click::Clicked),
    })]
    #[case::open_refuses_open(LatchRow {
        start: Latch::Open,
        message: LatchMessage::Open,
        next: Latch::Open,
        outcome: Err(LatchRejection::WhileOpen),
    })]
    #[case::closed_refuses_close(LatchRow {
        start: Latch::Closed,
        message: LatchMessage::Close,
        next: Latch::Closed,
        outcome: Err(LatchRejection::WhileClosed),
    })]
    fn update_writes_the_state_back_on_both_branches(#[case] row: LatchRow) {
        let mut slot = row.start;
        let outcome = slot.update(row.message);
        assert_eq!(slot, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
