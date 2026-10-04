#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

#[cfg(test)] extern crate self as widgets;

pub mod animation;
pub mod appearance;
mod braille;
pub mod card;
pub mod geometry;
mod key_hints;
pub mod milkdrop;
pub mod overlay;
pub mod pixels;
pub mod playlist;
pub mod primitive;
pub mod repaint;
pub mod scene;
pub mod screen;
pub mod spectrum;
pub mod status_line;
#[cfg(test)]
#[path = "../tests/unit/fixtures.rs"]
mod test_support;
pub mod theme;
pub mod toast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Playing {
    Yes,
    No,
}
