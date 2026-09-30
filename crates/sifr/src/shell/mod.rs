mod backdrop;
mod cover_fade;
mod frame_clock;
mod input;
mod motion;
mod painter;
mod view;
mod window_colors;

use std::io::{self, Stdout};

use crossterm::event::Event;
pub(crate) use input::ShellInput;
use ratatui::{Terminal, backend::CrosstermBackend};
use runtime::{FrameDue, FrameInput, Painted, Reaction, ShellEffect};
use terminal::ProbeAnswer;
pub(crate) use view::fallback_theme_file;

use crate::{
    shell::painter::{Painter, resized_area},
    startup::Look,
};

#[derive(Debug)]
pub(crate) struct Shell<'terminal> {
    terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
    frame: Painter,
}

impl<'terminal> Shell<'terminal> {
    pub(crate) fn new(
        terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
        look: Look,
    ) -> Result<Self, io::Error> {
        let size = terminal.size()?;
        let area = resized_area(size.width, size.height);
        Ok(Self {
            terminal,
            frame: Painter::new(area, look),
        })
    }

    pub(crate) fn apply_probe_answer(&mut self, answer: ProbeAnswer) {
        self.frame.apply_probe_answer(answer);
    }
}

impl runtime::Shell for Shell<'_> {
    type Input = ShellInput;
    type Error = io::Error;

    fn input(&mut self, event: Self::Input) -> Reaction {
        if let ShellInput::Terminal(Event::Resize(width, height)) = &event {
            self.frame.resized(resized_area(*width, *height));
        }
        input::message_for(event)
    }

    fn effect(&mut self, effect: ShellEffect) {
        self.frame.perform_effect(&effect);
    }

    fn frame_due(&self, view: &FrameInput<'_>) -> FrameDue {
        self.frame.frame_due(view)
    }

    fn paint(&mut self, view: FrameInput<'_>) -> Result<Painted, Self::Error> {
        self.frame.paint(self.terminal, view)
    }
}
