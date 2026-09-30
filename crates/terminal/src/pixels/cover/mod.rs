mod crossfade;
mod lifecycle;
mod milkdrop;
mod pixel;
mod protocol;
mod vinyl;
mod wash;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use config::{Animations, CoverStyle};
use image::RgbaImage;
use kernel::domain::Revision;
use raster::VinylColors;
use ratatui::text::Line;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use widgets::{CoverArt, FrameLayout, MilkdropColors, Playing, Spectrum};

use crate::pixels::cover::{
    lifecycle::{Cover, CoverRefresh, PixmapSource},
    milkdrop::{MilkdropCover, MilkdropParts},
};

/// A cover already decoded to pixels, handed in from outside the crate.
#[derive(Debug, Clone)]
pub struct DecodedCover {
    pub path: PathBuf,
    pub image: Arc<RgbaImage>,
}

/// A frame's cover art, owned outside `CoverRenderer` so the shell can borrow
/// it across a single `terminal.draw` call while `CoverRenderer::place` runs
/// inside.
#[derive(Debug, Clone)]
pub enum OwnedCoverArt {
    Missing,
    Image,
    Text(Arc<[Line<'static>]>),
}

impl OwnedCoverArt {
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

#[derive(Debug, Clone, Copy)]
pub struct CoverPlacement {
    pub layout: FrameLayout,
    pub fade: CoverFade,
    pub wash: CoverWash,
}

#[derive(Debug, Clone, Copy)]
pub struct CoverParts<'a> {
    pub key: CoverKey,
    pub look: CoverLook,
    pub moment: CoverMoment<'a>,
    pub placement: CoverPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoverKey {
    pub config_generation: Revision,
    pub theme_generation: Revision,
}

#[derive(Debug, Clone, Copy)]
pub struct CoverLook {
    pub style: CoverStyle,
    pub animations: Animations,
    pub vinyl: VinylColors,
    pub milkdrop: MilkdropColors,
}

#[derive(Debug, Clone, Copy)]
pub struct CoverMoment<'a> {
    pub clock: Duration,
    pub playing: Playing,
    pub track: Option<&'a Path>,
    pub bands: &'a Spectrum,
}

#[derive(Debug)]
pub(crate) struct CoverPixels {
    decoded: Option<DecodedCover>,
    active: Option<CoverStyle>,
    plain: Cover,
    vinyl: Cover,
    milkdrop: MilkdropCover,
}

impl Default for CoverPixels {
    fn default() -> Self {
        Self {
            decoded: None,
            active: None,
            plain: Cover::new(PixmapSource::Plain),
            vinyl: Cover::new(PixmapSource::Vinyl(Box::default())),
            milkdrop: MilkdropCover::default(),
        }
    }
}

impl CoverPixels {
    pub(crate) fn set_cover(&mut self, cover: DecodedCover) {
        self.decoded = Some(cover);
    }

    pub(crate) fn discard_protocol(&mut self) {
        self.plain.discard_protocol();
        self.vinyl.discard_protocol();
    }

    pub(crate) fn refresh(
        &mut self,
        picker: &Picker,
        sources: CoverParts<'_>,
    ) -> OwnedCoverArt {
        let CoverParts {
            key,
            look,
            moment,
            placement,
        } = sources;
        let CoverPlacement { layout, fade, wash } = placement;
        self.active = Some(look.style);
        let refresh = CoverRefresh {
            key,
            colors: look.vinyl,
            clock: moment.clock,
            animations: look.animations,
            layout,
            decoded: self.decoded.as_ref(),
            fade,
            wash,
        };
        match look.style {
            CoverStyle::Off => OwnedCoverArt::Missing,
            CoverStyle::Plain => self.plain.refresh(picker, refresh),
            CoverStyle::Vinyl => self.vinyl.refresh(picker, refresh),
            CoverStyle::Milkdrop => self.milkdrop.refresh(MilkdropParts {
                moment,
                colors: look.milkdrop,
                layout,
            }),
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
