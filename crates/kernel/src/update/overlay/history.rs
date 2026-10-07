use crate::{
    cmd::Cmd,
    domain::cursor_over::CursorOver,
    message::{HistoryRequest, Message, QueueRequest},
    update::machine::{Machine, Unhandled, replace},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub request: HistoryRequest,
    pub rows: usize,
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        history_message: HistoryMessage,
    ) -> Result<Cmd, Unhandled> {
        let HistoryMessage { request, rows } = history_message;
        match request {
            HistoryRequest::Navigate(direction) => {
                let moved = self.cursor.resize(rows).step(direction.sign());
                replace(&mut self.cursor, moved).map(|()| Cmd::none())
            }
            HistoryRequest::SelectFirst => {
                let moved = self.cursor.first();
                replace(&mut self.cursor, moved).map(|()| Cmd::none())
            }
            HistoryRequest::SelectLast => {
                let moved = self.cursor.resize(rows).last();
                replace(&mut self.cursor, moved).map(|()| Cmd::none())
            }
            HistoryRequest::Enqueue => {
                let selected = self.selected().get();
                (selected < rows)
                    .then(|| {
                        Cmd::message(Message::Queue(QueueRequest::ToggleHistoryEntry(
                            selected,
                        )))
                    })
                    .ok_or(Unhandled)
            }
        }
    }
}
