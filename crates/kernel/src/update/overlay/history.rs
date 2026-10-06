use crate::{
    cmd::Cmd,
    domain::{cursor_over::CursorOver, direction::Direction},
    message::{Message, QueueRequest},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMessage {
    Navigate { direction: Direction, len: usize },
    Top,
    Bottom(usize),
    Enqueue(usize),
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: HistoryMessage) -> Result<Cmd, Unhandled> {
        match message {
            HistoryMessage::Navigate { direction, len } => {
                self.resize(len);
                self.navigate(direction);
                Ok(Cmd::none())
            }
            HistoryMessage::Top => {
                self.cursor = self.cursor.first();
                Ok(Cmd::none())
            }
            HistoryMessage::Bottom(len) => {
                self.resize(len);
                self.cursor = self.cursor.last();
                Ok(Cmd::none())
            }
            HistoryMessage::Enqueue(len) => {
                let selected = self.selected().get();
                (selected < len)
                    .then(|| {
                        Cmd::message(Message::Queue(QueueRequest::EnqueueHistoryEntry(
                            selected,
                        )))
                    })
                    .ok_or(Unhandled)
            }
        }
    }
}
