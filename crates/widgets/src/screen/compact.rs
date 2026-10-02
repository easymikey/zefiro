use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{card::CompactCard, scene::Scene, screen::FrameLayout};

#[derive(Debug, Clone, Copy)]
pub struct CompactScreen<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
}

impl Widget for &CompactScreen<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let scene = self.scene;
        (&CompactCard {
            view: scene.card_view(),
            theme: scene.active_theme(),
            speed_chip: scene.look().appearance.speed_chip,
        })
            .render(self.layout.header, buffer);
    }
}
