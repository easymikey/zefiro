use audio::tap::SpectrumTap;
use kernel::{
    cmd::WindowColorsCmd,
    domain::{
        cue::Cue,
        geometry::{Cells, Pixels},
        model::Model,
        time::Moment,
    },
    message::{Message, PaintError},
};

use crate::latest::LatestReceivers;

pub trait Shell {
    type Input;
    type Error: std::error::Error + 'static;

    fn input(&mut self, input: Self::Input) -> Reaction;
    fn effect(&mut self, effect: ShellEffect);
    fn frame_due(&self, frame: &Frame<'_>) -> FrameDue;
    fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Self::Error>;
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
pub struct Frame<'a> {
    pub model: &'a Model,
    pub spectrum_tap: &'a SpectrumTap,
    pub latest_receivers: &'a LatestReceivers,
    pub now: Moment,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Painted {
    pub cover_side: Option<Pixels>,
    pub visible_rows: Cells,
    pub errors: Vec<PaintError>,
}
