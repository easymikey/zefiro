use std::{fmt, time::Duration};

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use widgets::{
    card::CardCover,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        lifecycle::{CoverFrame, CoverLifecycle, PixmapSource},
        pixmap::CellPixels,
    },
    scene::Scene,
};

use crate::pixels::cover::protocol::cover_protocol;

pub(crate) struct Cover {
    lifecycle: CoverLifecycle,
    picker: Picker,
    protocol: Option<StatefulProtocol>,
}

impl fmt::Debug for Cover {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cover")
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

fn cell_pixels(picker: &Picker) -> CellPixels {
    let font_size = picker.font_size();
    CellPixels {
        width: font_size.width,
        height: font_size.height,
    }
}

impl Cover {
    pub(crate) fn new(source: PixmapSource, picker: Picker) -> Self {
        Self {
            lifecycle: CoverLifecycle::new(source, cell_pixels(&picker)),
            picker,
            protocol: None,
        }
    }

    pub(crate) fn set_picker(&mut self, picker: Picker) {
        self.lifecycle.set_cell(cell_pixels(&picker));
        self.picker = picker;
        self.protocol = None;
    }

    pub(crate) fn set_cover(&mut self, decoded: CoverImage) {
        self.lifecycle.set_cover(decoded);
    }

    pub(crate) fn refresh(
        &mut self,
        scene: &Scene<'_>,
        refresh: CoverRefresh,
    ) -> CardCover {
        let update = self.lifecycle.refresh(scene, refresh);
        match update.frame {
            CoverFrame::Keep => {}
            CoverFrame::Forget => self.protocol = None,
            CoverFrame::Repaint(image) => {
                self.protocol = Some(cover_protocol(
                    &self.picker,
                    DynamicImage::ImageRgba8(image),
                ));
            }
        }
        update.art
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        self.protocol.as_mut()
    }

    pub(crate) fn motion(&self, now: Duration) -> CoverMotion {
        self.lifecycle.motion(now)
    }
}
