use std::time::Instant;

use audio::SpectrumTap;
use config::{AppearanceFile, AppearancePatch, ThemeFile};
use kernel::{Cue, Message, WindowColorsCmd, domain::Model};

use crate::library::cover::{CoverDecoded, CoverRequest};

pub trait Shell {
    type Input;
    type Error: std::error::Error + 'static;

    fn input(&mut self, event: Self::Input, model: &Model) -> Reaction;
    fn reloaded(&mut self, reload: Reload);
    fn effect(&mut self, effect: ShellEffect);
    fn cover(&mut self, decoded: CoverDecoded);
    fn frame_due(&self) -> FrameDue;
    fn paint(&mut self, view: View<'_>) -> Result<Painted, Self::Error>;
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub enum Reaction {
    Message(Message),
    Repaint,
    Ignored,
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub enum Reload {
    Theme(ThemeFile),
    Appearance(AppearanceFile),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellEffect {
    WindowColors(WindowColorsCmd),
    Animate(Cue),
    Appearance(AppearancePatch),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FrameDue {
    #[default]
    Settled,
    At(Instant),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Flow {
    #[default]
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub model: &'a Model,
    pub spectrum: &'a SpectrumTap,
    pub sleep_deadline: Option<Instant>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Painted {
    pub cover: Option<CoverRequest>,
}
