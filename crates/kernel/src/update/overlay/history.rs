use crate::{
    cmd::Cmd,
    domain::{cursor_over::CursorOver, direction::Direction},
    message::{Message, QueueRequest},
    update::machine::{Machine, Unhandled, move_cursor},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMessage {
    Navigate { direction: Direction, len: usize },
    SelectFirst,
    SelectLast { rows: usize },
    Enqueue(usize),
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        history_message: HistoryMessage,
    ) -> Result<Cmd, Unhandled> {
        match history_message {
            HistoryMessage::Navigate { direction, len } => {
                let moved = self.cursor.resize(len).step(direction.sign());
                move_cursor(&mut self.cursor, moved)
            }
            HistoryMessage::SelectFirst => {
                let moved = self.cursor.first();
                move_cursor(&mut self.cursor, moved)
            }
            HistoryMessage::SelectLast { rows } => {
                let moved = self.cursor.resize(rows).last();
                move_cursor(&mut self.cursor, moved)
            }
            HistoryMessage::Enqueue(len) => {
                let selected = self.selected().get();
                (selected < len)
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
