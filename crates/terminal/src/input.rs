use crossbeam_channel::Sender;
use crossterm::event::{self, Event};

pub fn run_input<T>(to_message: fn(Event) -> T, events: &Sender<T>) {
    while let Ok(event) = event::read() {
        if events.send(to_message(event)).is_err() {
            return;
        }
    }
}
