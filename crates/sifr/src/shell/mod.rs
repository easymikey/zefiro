mod backdrop;
mod cover_fade;
mod frame;
mod frame_clock;
mod input;
mod reload;
mod window_colors;

use std::io::Stdout;

use crossbeam_channel::Sender;
use crossterm::event::Event;
pub(crate) use input::ShellInput;
use kernel::domain::Model;
use ratatui::{Terminal, backend::CrosstermBackend};
use runtime::{CoverDecoded, FrameDue, Painted, Reaction, Reload, ShellEffect, View};
use terminal::ProbeAnswer;

use crate::{shell::frame::Frame, toast::ShellFailure};

#[derive(Debug)]
pub(crate) struct Shell<'terminal> {
    terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
    frame: Frame,
    failures: Sender<ShellInput>,
}

impl<'terminal> Shell<'terminal> {
    pub(crate) fn new(
        terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
        failures: Sender<ShellInput>,
    ) -> Self {
        Self {
            terminal,
            frame: Frame::new(),
            failures,
        }
    }

    pub(crate) fn adopt(&mut self, answer: ProbeAnswer) {
        self.frame.adopt(answer);
    }

    fn report(&self, failure: ShellFailure) {
        let _ = self.failures.send(ShellInput::Failed(failure));
    }
}

impl runtime::Shell for Shell<'_> {
    type Input = ShellInput;
    type Error = std::io::Error;

    fn input(&mut self, event: Self::Input, _model: &Model) -> Reaction {
        if let ShellInput::Terminal(Event::Resize(_, _)) = &event {
            self.frame.mark_resized();
        }
        input::message_for(event)
    }

    fn reloaded(&mut self, reload: Reload) {
        if let Some(failure) = self.frame.reloaded(reload) {
            self.report(failure);
        }
    }

    fn effect(&mut self, effect: ShellEffect) {
        if let Some(failure) = self.frame.effect(&effect) {
            self.report(failure);
        }
    }

    fn cover(&mut self, decoded: CoverDecoded) {
        if let Some(failure) = self.frame.cover(decoded) {
            self.report(failure);
        }
    }

    fn frame_due(&self) -> FrameDue {
        self.frame.frame_due()
    }

    fn paint(&mut self, view: View<'_>) -> Result<Painted, Self::Error> {
        let painted = self.frame.paint(self.terminal, view)?;
        if let Some(failure) = self.frame.take_settled_failure() {
            self.report(failure);
        }
        Ok(painted)
    }
}
