use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    style::Style,
    widgets::{Block, Widget},
};

use crate::{
    card::{CardCover, CardView, CardWidget, compact::CompactCardWidget},
    key_hints::{KeyHintsView, KeyHintsWidget},
    overlay::layer::{OverlayView, OverlayWidget},
    playlist::pane::PlaylistWidget,
    primitive::canvas::Canvas,
    scene::Scene,
    screen::{
        breakpoint::Breakpoint,
        frame_layout::FrameLayout,
        minimal::MinimalScreenWidget,
        too_small::TooSmallWidget,
    },
    toast::ToastWidget,
};

#[derive(Debug, Clone, Copy)]
pub struct ScreenWidget<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
    pub cover_art: &'a CardCover,
}

impl Widget for &ScreenWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.scene.active_theme();
        let colors = theme.colors();
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.text),
            )
            .render(area, buffer);
        let layout = self.layout;
        match layout.breakpoint {
            Breakpoint::TooSmall => {
                let breakpoints = self.scene.presentation.appearance.breakpoints;
                (&TooSmallWidget {
                    theme,
                    minimum: Size::new(
                        breakpoints.min_width.0,
                        breakpoints.min_height.0,
                    ),
                })
                    .render(layout.screen, buffer);
                return;
            }
            Breakpoint::Minimal => (&MinimalScreenWidget {
                view: CardView::from_scene(&self.scene),
                theme,
                speed_chip: self.scene.settings.appearance.speed_chip,
            })
                .render(layout.screen, buffer),
            Breakpoint::Full => self.paint_card(buffer),
            Breakpoint::Compact => (&CompactCardWidget {
                view: CardView::from_scene(&self.scene),
                theme,
                speed_chip: self.scene.settings.appearance.speed_chip,
            })
                .render(layout.header, buffer),
        }
        self.paint_lists(buffer);
        self.paint_layers(buffer);
    }
}

impl ScreenWidget<'_> {
    fn paint_card(&self, buffer: &mut Buffer) {
        let Some(metrics) = self.layout.card else {
            return;
        };
        let scene = self.scene;
        CardWidget {
            view: CardView::from_scene(&scene),
            theme: scene.active_theme(),
            cell_aspect: scene.presentation.cell_aspect,
            cover_sizing: scene.cover_sizing(),
            appearance: scene.settings.appearance,
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

    fn paint_lists(&self, buffer: &mut Buffer) {
        let scene = self.scene;
        let theme = scene.active_theme();
        if let Some(areas) = self.layout.playlist {
            PlaylistWidget {
                view: crate::playlist::view::PlaylistView::from_scene(&scene),
                theme,
            }
            .paint(&areas, buffer);
        }
        if let Some(hints) = self.layout.key_hints {
            (&KeyHintsWidget {
                theme,
                view: KeyHintsView::from_scene(&scene),
            })
                .render(hints, buffer);
        }
    }

    fn paint_layers(&self, buffer: &mut Buffer) {
        let screen = self.layout.screen;
        if let Some(areas) = self.layout.overlay {
            OverlayWidget::placed(
                OverlayView::from_scene(&self.scene),
                self.layout,
                self.scene.cover_mode(),
            )
            .paint(
                areas,
                Canvas {
                    area: screen,
                    buffer: &mut *buffer,
                },
            );
        }
        if let Some((toast, areas)) =
            ToastWidget::from_scene(&self.scene).zip(self.layout.toast)
        {
            toast.paint(
                areas,
                Canvas {
                    area: screen,
                    buffer,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::toast::Toast;
    use ratatui::layout::Rect;

    use crate::{
        card::CardCover,
        scene::{PixelPath, Scene},
        screen::{frame_layout::FrameLayout, root::ScreenWidget},
        test_support::{SceneSources, model_with_tracks, rendered},
    };

    fn frame(scene: Scene<'_>, cover_art: &CardCover, size: (u16, u16)) -> String {
        let (width, height) = size;
        let layout = FrameLayout::from_scene(&scene, Rect::new(0, 0, width, height));
        rendered(width, height, |frame| {
            frame.render_widget(
                &ScreenWidget {
                    scene,
                    layout: &layout,
                    cover_art,
                },
                frame.area(),
            );
        })
        .to_string()
    }

    #[test]
    fn a_full_frame_paints_the_card_the_playlist_and_the_key_hints() {
        let sources = SceneSources::new(model_with_tracks(3));
        insta::assert_snapshot!(frame(sources.scene(), &CardCover::Missing, (80, 24)));
    }

    #[test]
    fn the_cover_art_decides_whether_the_placeholder_is_painted() {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.pixel_path = PixelPath::Protocol;
        let scene = sources.scene();
        let missing = frame(scene, &CardCover::Missing, (80, 24));
        let image = frame(scene, &CardCover::Image, (80, 24));
        assert!(missing.contains("No cover"), "got {missing}");
        assert!(!image.contains("No cover"), "got {image}");
    }

    #[test]
    fn a_short_terminal_paints_the_compact_card() {
        let sources = SceneSources::new(model_with_tracks(3));
        let text = frame(sources.scene(), &CardCover::Missing, (80, 16));
        assert!(text.contains("No track"), "got {text}");
        assert!(text.contains("song00"), "got {text}");
    }

    #[test]
    fn a_terminal_below_the_minimum_shows_only_the_notice() {
        let sources = SceneSources::new(model_with_tracks(3));
        let text = frame(sources.scene(), &CardCover::Missing, (40, 10));
        assert!(text.contains("Terminal too small."), "got {text}");
        assert!(!text.contains("song00"), "got {text}");
    }

    #[test]
    fn the_toast_is_painted_over_the_frame() {
        let mut model = model_with_tracks(3);
        model.workspace.toasts = vec![Toast::info("Saved")];
        let sources = SceneSources::new(model);
        let text = frame(sources.scene(), &CardCover::Missing, (80, 24));
        assert!(text.contains("Saved"), "got {text}");
    }
}
