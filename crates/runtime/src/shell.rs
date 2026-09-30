use audio::SpectrumTap;
use kernel::{Cue, Message, Moment, WindowColorsCmd, domain::Model};

use crate::{cells::Receivers, library::cover::CoverRequest};

pub trait Shell {
    type Input;
    type Error: std::error::Error + 'static;

    fn input(&mut self, event: Self::Input) -> Reaction;
    fn effect(&mut self, effect: ShellEffect);
    fn frame_due(&self, view: &FrameInput<'_>) -> FrameDue;
    fn paint(&mut self, view: FrameInput<'_>) -> Result<Painted, Self::Error>;
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub enum Reaction {
    Message(Message),
    Repaint,
    Ignored,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellEffect {
    WindowColors(WindowColorsCmd),
    Animate(Cue),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FrameDue {
    #[default]
    Settled,
    At(Moment),
}

#[derive(Debug, Clone, Copy)]
pub struct FrameInput<'a> {
    pub model: &'a Model,
    pub spectrum: &'a SpectrumTap,
    pub cells: &'a Receivers,
    pub sleep_deadline: Option<Moment>,
    pub now: Moment,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Painted {
    pub cover: Option<CoverRequest>,
    pub visible_rows: Option<usize>,
    pub failures: Vec<Message>,
}
