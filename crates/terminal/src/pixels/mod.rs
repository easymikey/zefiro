mod cover;

use std::time::Duration;

use kernel::domain::appearance::CoverMode;
use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use ratatui_image::{StatefulImage, picker::Picker};
use widgets::{
    CardCover,
    CoverImage,
    CoverMotion,
    CoverRefresh,
    FrameLayout,
    MilkdropCover,
    OverlayAreas,
    PixmapSource,
    Scene,
};

use crate::pixels::cover::Cover;

#[derive(Debug)]
pub struct CoverPainter {
    active: Option<CoverMode>,
    plain: Cover,
    vinyl: Cover,
    milkdrop: MilkdropCover,
}

impl CoverPainter {
    #[must_use]
    pub fn new(picker: Picker) -> Self {
        Self {
            active: None,
            plain: Cover::new(PixmapSource::Plain, picker.clone()),
            vinyl: Cover::new(PixmapSource::Vinyl(Box::default()), picker),
            milkdrop: MilkdropCover::default(),
        }
    }

    pub fn set_picker(&mut self, picker: Picker) {
        self.plain.set_picker(picker.clone());
        self.vinyl.set_picker(picker);
    }

    pub fn set_cover(&mut self, cover: CoverImage) {
        self.plain.set_cover(cover.clone());
        self.vinyl.set_cover(cover);
    }

    pub fn refresh(&mut self, scene: &Scene<'_>, refresh: CoverRefresh) -> CardCover {
        let mode = scene.cover_mode();
        self.active = Some(mode);
        match mode {
            CoverMode::Off => CardCover::Missing,
            CoverMode::Plain => self.plain.refresh(scene, refresh),
            CoverMode::Vinyl => self.vinyl.refresh(scene, refresh),
            CoverMode::Milkdrop => self.milkdrop.refresh(scene, refresh.layout.cover),
        }
    }

    #[must_use]
    pub fn cover_motion(&self, now: Duration) -> CoverMotion {
        match (self.plain.motion(now), self.vinyl.motion(now)) {
            (CoverMotion::Animating, _) | (_, CoverMotion::Animating) => {
                CoverMotion::Animating
            }
            (CoverMotion::Still, CoverMotion::Still) => CoverMotion::Still,
        }
    }

    pub fn paint(&mut self, buffer: &mut Buffer, layout: &FrameLayout) {
        let Some(rect) = layout.cover else {
            return;
        };
        if hidden_by_overlay(rect, layout) {
            return;
        }
        let protocol = match self.active {
            Some(CoverMode::Plain) => self.plain.protocol_mut(),
            Some(CoverMode::Vinyl) => self.vinyl.protocol_mut(),
            Some(CoverMode::Milkdrop | CoverMode::Off) | None => None,
        };
        let Some(protocol) = protocol else {
            return;
        };
        StatefulWidget::render(StatefulImage::default(), rect, buffer, protocol);
    }
}

fn hidden_by_overlay(rect: Rect, layout: &FrameLayout) -> bool {
    let overlay = layout.overlay.map(OverlayAreas::outer);
    let toast = layout.toast.map(|toast| toast.painted);
    [overlay, toast]
        .into_iter()
        .flatten()
        .any(|painted| !painted.intersection(rect).is_empty())
}
