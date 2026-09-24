mod crossfade;
mod milkdrop;
mod pixel;
mod protocol;
mod vinyl;
mod wash;

use std::{path::PathBuf, time::Duration};

use config::CoverStyle;
use image::RgbaImage;
use ratatui::text::Line;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use widgets::{CoverArt, FrameLayout, Scene};

use crate::pixels::cover::{
    milkdrop::{MilkdropCover, MilkdropSources},
    pixel::{PlainCover, PlainSources},
    vinyl::{VinylCover, VinylSources},
};

/// A cover already decoded to pixels, handed in from outside the crate.
#[derive(Debug, Clone)]
pub struct DecodedCover {
    pub path: PathBuf,
    pub image: RgbaImage,
}

/// A frame's cover art, owned outside `Pixels` so the shell can borrow it
/// across a single `terminal.draw` call while `Pixels::place` runs inside.
#[derive(Debug, Clone)]
pub enum CoverArtOwner {
    Missing,
    Image,
    Text(Vec<Line<'static>>),
}

impl CoverArtOwner {
    #[must_use]
    pub fn as_cover_art(&self) -> CoverArt<'_> {
        match self {
            Self::Missing => CoverArt::Missing,
            Self::Image => CoverArt::Image,
            Self::Text(lines) => CoverArt::Text(lines),
        }
    }
}

/// Whether the plain cover is mid-crossfade, for callers that must keep
/// asking for animation frames while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverMotion {
    Crossfading,
    Still,
}

/// Whether the caller grants the plain cover permission to begin a
/// crossfade when it installs a new pixmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverFade {
    Allowed,
    Withheld,
}

/// The screen's theme wash, for a cover that must keep showing its outgoing
/// theme's colours behind the reveal until the wash catches up with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverWash {
    Running { progress: f32, screen_width: u16 },
    Idle,
}

/// What a frame's refresh needs to place the cover, plus the caller's
/// crossfade permission for the plain cover and the screen's theme wash.
#[derive(Debug, Clone, Copy)]
pub struct CoverSources<'a> {
    pub scene: Scene<'a>,
    pub layout: FrameLayout,
    pub fade: CoverFade,
    pub wash: CoverWash,
}

#[derive(Default, Debug)]
pub(crate) struct CoverPixels {
    decoded: Option<DecodedCover>,
    active: Option<CoverStyle>,
    plain: PlainCover,
    vinyl: VinylCover,
    milkdrop: MilkdropCover,
}

impl CoverPixels {
    pub(crate) fn accept(&mut self, cover: DecodedCover) {
        self.decoded = Some(cover);
    }

    pub(crate) fn discard_protocol(&mut self) {
        self.plain.discard_protocol();
        self.vinyl.discard_protocol();
    }

    pub(crate) fn refresh(
        &mut self,
        picker: &Picker,
        sources: CoverSources<'_>,
    ) -> CoverArtOwner {
        let CoverSources {
            scene,
            layout,
            fade,
            wash,
        } = sources;
        let style = scene.cover_style();
        self.active = Some(style);
        let decoded = self.decoded.as_ref();
        match style {
            CoverStyle::Off => CoverArtOwner::Missing,
            CoverStyle::Plain => self.plain.refresh(
                picker,
                PlainSources {
                    scene,
                    layout,
                    decoded,
                    fade,
                    wash,
                },
            ),
            CoverStyle::Vinyl => self.vinyl.refresh(
                picker,
                VinylSources {
                    scene,
                    layout,
                    decoded,
                    fade,
                    wash,
                },
            ),
            CoverStyle::Milkdrop => {
                self.milkdrop.refresh(MilkdropSources { scene, layout })
            }
        }
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        match self.active? {
            CoverStyle::Plain => self.plain.protocol_mut(),
            CoverStyle::Vinyl => self.vinyl.protocol_mut(),
            CoverStyle::Milkdrop | CoverStyle::Off => None,
        }
    }

    pub(crate) fn motion(&self, now: Duration) -> CoverMotion {
        match (self.plain.motion(now), self.vinyl.motion(now)) {
            (CoverMotion::Crossfading, _) | (_, CoverMotion::Crossfading) => {
                CoverMotion::Crossfading
            }
            (CoverMotion::Still, CoverMotion::Still) => CoverMotion::Still,
        }
    }
}
