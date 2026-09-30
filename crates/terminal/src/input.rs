use crossbeam_channel::Sender;
use crossterm::event::{self, Event};

#[derive(Debug, Clone, Copy)]
pub struct InputLoop;

impl InputLoop {
    pub fn run<T>(self, to_message: fn(Event) -> T, events: &Sender<T>) {
        loop {
            match event::read() {
                Ok(event) => {
                    if events.send(to_message(event)).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}
