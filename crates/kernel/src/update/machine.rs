#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a state machine — it has no `Machine` impl",
    label = "missing `impl Machine for {Self}`",
    note = "every (state, message) pair is a row of this machine's transition table"
)]
pub trait Machine {
    type Message;
    type Error;
    type Effect;

    fn transition(
        &mut self,
        message: Self::Message,
    ) -> Result<Self::Effect, Self::Error>;
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::update::machine::Machine;

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
    enum LatchError {
        WhileOpen,
        WhileClosed,
    }

    #[derive(Debug, PartialEq)]
    enum Click {
        Clicked,
    }

    impl Machine for Latch {
        type Message = LatchMessage;
        type Error = LatchError;
        type Effect = Click;

        fn transition(&mut self, message: LatchMessage) -> Result<Click, LatchError> {
            match (&*self, message) {
                (Latch::Open, LatchMessage::Close) => {
                    *self = Latch::Closed;
                    Ok(Click::Clicked)
                }
                (Latch::Closed, LatchMessage::Open) => {
                    *self = Latch::Open;
                    Ok(Click::Clicked)
                }
                (Latch::Open, LatchMessage::Open) => Err(LatchError::WhileOpen),
                (Latch::Closed, LatchMessage::Close) => Err(LatchError::WhileClosed),
            }
        }
    }

    struct LatchRow {
        start: Latch,
        message: LatchMessage,
        next: Latch,
        outcome: Result<Click, LatchError>,
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
        outcome: Err(LatchError::WhileOpen),
    })]
    #[case::closed_refuses_close(LatchRow {
        start: Latch::Closed,
        message: LatchMessage::Close,
        next: Latch::Closed,
        outcome: Err(LatchError::WhileClosed),
    })]
    fn transition_writes_only_on_success(#[case] row: LatchRow) {
        let mut slot = row.start;
        let outcome = slot.transition(row.message);
        assert_eq!(slot, row.next);
        assert_eq!(outcome, row.outcome);
    }
}
