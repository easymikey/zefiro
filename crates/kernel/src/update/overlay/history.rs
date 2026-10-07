use crate::{
    cmd::Cmd,
    domain::cursor_over::CursorOver,
    message::{HistoryRequest, Message, QueueRequest},
    update::machine::{Machine, Unhandled, move_cursor},
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
                move_cursor(&mut self.cursor, moved)
            }
            HistoryRequest::SelectFirst => {
                let moved = self.cursor.first();
                move_cursor(&mut self.cursor, moved)
            }
            HistoryRequest::SelectLast => {
                let moved = self.cursor.resize(rows).last();
                move_cursor(&mut self.cursor, moved)
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
