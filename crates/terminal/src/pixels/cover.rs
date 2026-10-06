use std::{fmt, time::Duration};

use image::DynamicImage;
use ratatui_image::{
    picker::{Capability, Picker, ProtocolType},
    protocol::{StatefulProtocol, StatefulProtocolType, kitty::StatefulKitty},
};
use widgets::{
    card::CardCover,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        CoverWash,
        CrossfadePermit,
        lifecycle::{CoverFrame, CoverLifecycle, PixmapSource},
        pixmap::CellPixels,
    },
    scene::Scene,
};

const COVER_KITTY_ID: u32 = 1;

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

impl Cover {
    pub(crate) fn new(
        pixmap_source: PixmapSource,
        picker: Picker,
        cell_pixels: CellPixels,
    ) -> Self {
        Self {
            lifecycle: CoverLifecycle::new(pixmap_source, cell_pixels),
            picker,
            protocol: None,
        }
    }

    pub(crate) fn set_cover(&mut self, cover_image: CoverImage) {
        self.lifecycle.set_cover(cover_image);
    }

    pub(crate) fn refresh(
        &mut self,
        scene: &Scene<'_>,
        refresh: CoverRefresh,
    ) -> CardCover {
        let refresh = match self.picker.protocol_type() {
            ProtocolType::Halfblocks => refresh,
            ProtocolType::Sixel | ProtocolType::Kitty | ProtocolType::Iterm2 => {
                CoverRefresh {
                    crossfade_permit: CrossfadePermit::Withheld,
                    wash: CoverWash::Idle,
                    ..refresh
                }
            }
        };
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
        update.card_cover
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        self.protocol.as_mut()
    }

    pub(crate) fn motion(&self, since_first_paint: Duration) -> CoverMotion {
        self.lifecycle.motion(since_first_paint)
    }
}

fn cover_protocol(picker: &Picker, image: DynamicImage) -> StatefulProtocol {
    if picker.protocol_type() != ProtocolType::Kitty {
        return picker.new_resize_protocol(image);
    }
    let compress = picker
        .capabilities()
        .contains(&Capability::KittyCompression);
    let protocol_type = StatefulProtocolType::Kitty(StatefulKitty::new(
        COVER_KITTY_ID,
        picker.tmux_detected(),
        compress,
    ));
    StatefulProtocol::new(image, picker.font_size(), None, protocol_type)
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, Rgba, RgbaImage};
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::Color,
        widgets::StatefulWidget,
    };
    use ratatui_image::{
        StatefulImage,
        picker::{Picker, ProtocolType},
        protocol::{StatefulProtocol, StatefulProtocolType},
    };

    use crate::pixels::cover::cover_protocol;

    fn image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
    }

    fn other_image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([9, 8, 7, 255])))
    }

    fn rendered_foreground(protocol: &mut StatefulProtocol) -> Color {
        let rect = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(rect);
        StatefulWidget::render(StatefulImage::default(), rect, &mut buffer, protocol);
        buffer[(0, 0)].style().fg.unwrap_or(Color::Reset)
    }

    #[test]
    fn a_halfblocks_picker_keeps_the_picker_resize_protocol() {
        let picker = Picker::halfblocks();
        let protocol = cover_protocol(&picker, image());
        assert!(matches!(
            protocol.protocol_type(),
            StatefulProtocolType::Halfblocks(_)
        ));
    }

    #[test]
    fn a_kitty_repaint_renders_under_the_same_id() {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut first = cover_protocol(&picker, image());
        let mut second = cover_protocol(&picker, other_image());
        assert_eq!(
            rendered_foreground(&mut first),
            rendered_foreground(&mut second)
        );
    }
}
