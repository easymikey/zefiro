use crossbeam_channel::Sender;
use crossterm::event::{self, Event};

#[derive(Debug, Clone, Copy)]
pub struct InputLoop;

impl InputLoop {
    pub fn run(self, events: &Sender<Event>) {
        loop {
            match event::read() {
                Ok(event) => {
                    if events.send(event).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}
