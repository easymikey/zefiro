use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    card::{Card, CoverArt},
    primitive::canvas::Canvas,
    scene::Scene,
    screen::FrameLayout,
};

#[derive(Debug, Clone, Copy)]
pub struct FullScreen<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
    pub cover_art: &'a CoverArt,
}

impl Widget for &FullScreen<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let Some(metrics) = self.layout.card else {
            return;
        };
        let scene = self.scene;
        Card {
            view: scene.card_view(),
            theme: scene.active_theme(),
            cell_aspect: scene.cell_aspect,
            cover_sizing: scene.cover_sizing(),
            appearance: scene.appearance.appearance(),
            cover_art: self.cover_art,
        }
        .render_in(
            &metrics,
            Canvas {
                area: self.layout.header,
                buffer,
            },
        );
    }
}
