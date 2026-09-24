use image::DynamicImage;
use ratatui_image::{
    picker::{Capability, Picker, ProtocolType},
    protocol::{StatefulProtocol, StatefulProtocolType, kitty::StatefulKitty},
};

/// The kitty image id the cover slot always transmits under. Reusing it lets
/// a re-transmit replace the previously stored image instead of leaving an
/// orphan in the terminal's image store.
const COVER_KITTY_ID: u32 = 1;

/// Builds a protocol for the cover slot. For the kitty protocol this pins the
/// image id so a repaint replaces the stored image rather than allocating a
/// new one; every other protocol keeps the picker's own resize protocol.
pub(crate) fn cover_protocol(picker: &Picker, image: DynamicImage) -> StatefulProtocol {
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

    use crate::pixels::cover::protocol::cover_protocol;

    fn image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
    }

    fn other_image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([9, 8, 7, 255])))
    }

    fn rendered_fg(protocol: &mut StatefulProtocol) -> Color {
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
        assert_eq!(rendered_fg(&mut first), rendered_fg(&mut second));
    }
}
