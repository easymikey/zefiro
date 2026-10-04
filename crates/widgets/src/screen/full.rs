use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::{
    card::{CardCover, CardWidget},
    primitive::canvas::Canvas,
    scene::Scene,
    screen::FrameLayout,
};

#[derive(Debug, Clone, Copy)]
pub struct FullScreenWidget<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
    pub cover_art: &'a CardCover,
}

impl Widget for &FullScreenWidget<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let Some(metrics) = self.layout.card else {
            return;
        };
        let scene = self.scene;
        CardWidget {
            view: crate::card::CardView::from_scene(&scene),
            theme: scene.active_theme(),
            cell_aspect: scene.cell_aspect,
            cover_sizing: scene.cover_sizing(),
            appearance: scene.appearance().settings,
            cover_art: self.cover_art,
        }
        .paint(
            &metrics,
            Canvas {
                area: self.layout.header,
                buffer,
            },
        );
    }
}
