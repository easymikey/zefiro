use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{card::CompactCardWidget, scene::Scene, screen::FrameLayout};

#[derive(Debug, Clone, Copy)]
pub struct CompactScreenWidget<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
}

impl Widget for &CompactScreenWidget<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let scene = self.scene;
        (&CompactCardWidget {
            view: crate::card::CardView::from_scene(&scene),
            theme: scene.active_theme(),
            speed_chip: scene.appearance().settings.speed_chip,
        })
            .render(self.layout.header, buffer);
    }
}
