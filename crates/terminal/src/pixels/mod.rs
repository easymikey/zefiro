mod cover;

use std::time::Duration;

use kernel::domain::appearance::CoverMode;
use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use ratatui_image::{StatefulImage, picker::Picker};
use widgets::{
    card::CardCover,
    milkdrop::cover::MilkdropCover,
    overlay::modal::placement::OverlayAreas,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        lifecycle::PixmapSource,
        pixmap::CellPixels,
    },
    scene::Scene,
    screen::frame_layout::FrameLayout,
};

use crate::pixels::cover::Cover;

#[derive(Debug)]
pub struct CoverPainter {
    active: CoverMode,
    plain: Cover,
    vinyl: Cover,
    milkdrop: MilkdropCover,
}

impl CoverPainter {
    #[must_use]
    pub fn new(picker: Picker, cell: CellPixels) -> Self {
        Self {
            active: CoverMode::Off,
            plain: Cover::new(PixmapSource::Plain, picker.clone(), cell),
            vinyl: Cover::new(PixmapSource::Vinyl(Box::default()), picker, cell),
            milkdrop: MilkdropCover::default(),
        }
    }

    pub fn set_cover(&mut self, cover: CoverImage) {
        self.plain.set_cover(cover.clone());
        self.vinyl.set_cover(cover);
    }

    pub fn refresh(&mut self, scene: &Scene<'_>, refresh: CoverRefresh) -> CardCover {
        let mode = scene.cover_mode();
        self.active = mode;
        match mode {
            CoverMode::Off => CardCover::Missing,
            CoverMode::Plain => self.plain.refresh(scene, refresh),
            CoverMode::Vinyl => self.vinyl.refresh(scene, refresh),
            CoverMode::Milkdrop => self.milkdrop.refresh(scene, refresh.cover),
        }
    }

    #[must_use]
    pub fn cover_motion(&self, now: Duration) -> CoverMotion {
        match self.active {
            CoverMode::Plain => self.plain.motion(now),
            CoverMode::Vinyl => self.vinyl.motion(now),
            CoverMode::Milkdrop | CoverMode::Off => CoverMotion::Still,
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
            CoverMode::Plain => self.plain.protocol_mut(),
            CoverMode::Vinyl => self.vinyl.protocol_mut(),
            CoverMode::Milkdrop | CoverMode::Off => None,
        };
        let Some(protocol) = protocol else {
            return;
        };
        StatefulWidget::render(StatefulImage::default(), rect, buffer, protocol);
    }
}

fn hidden_by_overlay(rect: Rect, layout: &FrameLayout) -> bool {
    let overlay = layout.overlay.map(OverlayAreas::outer);
    let toast = layout.toast;
    [overlay, toast]
        .into_iter()
        .flatten()
        .any(|painted| !painted.intersection(rect).is_empty())
}
