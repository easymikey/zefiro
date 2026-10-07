mod cover;

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
        lifecycle::{CoverLifecycle, PixmapSource},
        pixmap::CellPixels,
    },
    scene::Scene,
    screen::frame_layout::FrameLayout,
};

use crate::pixels::cover::Cover;

#[derive(Debug)]
pub struct CoverPainter {
    cover_mode: CoverMode,
    plain_cover: Cover,
    vinyl_cover: Cover,
    milkdrop_cover: MilkdropCover,
}

impl CoverPainter {
    #[must_use]
    pub fn new(picker: Picker, cell_pixels: CellPixels) -> Self {
        Self {
            cover_mode: CoverMode::Off,
            plain_cover: Cover::new(
                CoverLifecycle::new(PixmapSource::Plain, cell_pixels),
                picker.clone(),
            ),
            vinyl_cover: Cover::new(
                CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell_pixels),
                picker,
            ),
            milkdrop_cover: MilkdropCover::default(),
        }
    }

    pub fn set_cover(&mut self, cover_image: CoverImage) {
        self.plain_cover.set_cover(cover_image.clone());
        self.vinyl_cover.set_cover(cover_image);
    }

    pub fn refresh(
        &mut self,
        scene: &Scene<'_>,
        cover_area: Option<Rect>,
    ) -> CardCover {
        let cover_mode = scene.cover_mode();
        self.cover_mode = cover_mode;
        match cover_mode {
            CoverMode::Off => CardCover::Missing,
            CoverMode::Plain => self.plain_cover.refresh(scene, cover_area),
            CoverMode::Vinyl => self.vinyl_cover.refresh(scene, cover_area),
            CoverMode::Milkdrop => self.milkdrop_cover.refresh(scene, cover_area),
        }
    }

    #[must_use]
    pub fn motion(&self) -> CoverMotion {
        match self.cover_mode {
            CoverMode::Plain => self.plain_cover.motion(),
            CoverMode::Vinyl => self.vinyl_cover.motion(),
            CoverMode::Milkdrop | CoverMode::Off => CoverMotion::Still,
        }
    }

    pub fn paint(&mut self, buffer: &mut Buffer, layout: &FrameLayout<'_>) {
        let Some(rect) = layout.cover_area else {
            return;
        };
        if is_hidden_by_overlay(rect, layout) {
            return;
        }
        let protocol = match self.cover_mode {
            CoverMode::Plain => self.plain_cover.protocol_mut(),
            CoverMode::Vinyl => self.vinyl_cover.protocol_mut(),
            CoverMode::Milkdrop | CoverMode::Off => None,
        };
        let Some(protocol) = protocol else {
            return;
        };
        StatefulWidget::render(StatefulImage::default(), rect, buffer, protocol);
    }
}

fn is_hidden_by_overlay(rect: Rect, layout: &FrameLayout<'_>) -> bool {
    let overlay = layout.overlay_areas.map(OverlayAreas::outer);
    let toast = layout.toast;
    [overlay, toast]
        .into_iter()
        .flatten()
        .any(|painted| !painted.intersection(rect).is_empty())
}
