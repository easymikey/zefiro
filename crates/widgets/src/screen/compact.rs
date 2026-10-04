use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    card::compact::CompactCardWidget,
    scene::Scene,
    screen::frame_layout::FrameLayout,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompactScreenWidget<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: &'a FrameLayout,
}

impl Widget for &CompactScreenWidget<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let scene = self.scene;
        (&CompactCardWidget {
            view: crate::card::CardView::from_scene(&scene),
            theme: scene.active_theme(),
            speed_chip: scene.appearance_settings().speed_chip,
        })
            .render(self.layout.header, buffer);
    }
}
