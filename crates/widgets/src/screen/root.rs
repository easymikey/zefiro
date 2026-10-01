use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    style::Style,
    widgets::{Block, Widget},
};

use crate::{
    card::CoverArt,
    key_hints::KeyHintsLine,
    overlay::layer::OverlayLayer,
    playlist::PlaylistPane,
    primitive::canvas::Canvas,
    scene::Scene,
    screen::{
        Breakpoint,
        CompactScreen,
        FrameLayout,
        FullScreen,
        MinimalScreen,
        TooSmallNotice,
    },
    theme::Role,
};

#[derive(Debug, Clone, Copy)]
pub struct Screen<'a> {
    pub scene: Scene<'a>,
    pub layout: &'a FrameLayout,
    pub cover_art: &'a CoverArt,
}

impl Widget for &Screen<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.scene.active_theme();
        Block::new()
            .style(
                Style::default()
                    .bg(theme.role(Role::WindowBackground))
                    .fg(theme.role(Role::Text)),
            )
            .render(area, buffer);
        let layout = self.layout;
        match layout.breakpoint {
            Breakpoint::TooSmall => {
                let breakpoints = self.scene.appearance.layout;
                (&TooSmallNotice {
                    theme,
                    minimum: Size::new(breakpoints.min_columns, breakpoints.min_rows),
                })
                    .render(layout.screen, buffer);
                return;
            }
            Breakpoint::Minimal => (&MinimalScreen {
                view: self.scene.card_view(),
                theme,
                speed_chip: self.scene.appearance.card.speed_chip,
            })
                .render(layout.screen, buffer),
            Breakpoint::Full => (&FullScreen {
                scene: self.scene,
                layout: self.layout,
                cover_art: self.cover_art,
            })
                .render(layout.screen, buffer),
            Breakpoint::Compact => (&CompactScreen {
                scene: self.scene,
                layout: self.layout,
            })
                .render(layout.screen, buffer),
        }
        self.render_lists(buffer);
        self.render_layers(buffer);
    }
}

impl Screen<'_> {
    fn render_lists(&self, buffer: &mut Buffer) {
        let scene = self.scene;
        let theme = scene.active_theme();
        if let Some(areas) = self.layout.playlist {
            PlaylistPane {
                view: scene.playlist_view(),
                theme,
            }
            .render_in(&areas, buffer);
        }
        if let Some(hints) = self.layout.key_hints {
            (&KeyHintsLine {
                theme,
                content: scene.key_hints(),
            })
                .render(hints, buffer);
        }
    }

    fn render_layers(&self, buffer: &mut Buffer) {
        let screen = self.layout.screen;
        if let Some(areas) = self.layout.overlay {
            OverlayLayer::placed(
                self.scene.overlay_content(),
                self.layout,
                self.scene.cover_style(),
            )
            .render_in(
                areas,
                Canvas {
                    area: screen,
                    buffer: &mut *buffer,
                },
            );
        }
        if let Some((toast, areas)) = self.scene.toast_card().zip(self.layout.toast) {
            toast.render_in(
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
    use kernel::domain::Toast;
    use ratatui::layout::Rect;

    use crate::{
        card::CoverArt,
        scene::{PixelPath, Scene},
        screen::{FrameLayout, Screen},
        test_support::{SceneSources, model_with_tracks, rendered},
    };

    fn frame(scene: Scene<'_>, cover_art: &CoverArt, size: (u16, u16)) -> String {
        let (width, height) = size;
        let layout =
            FrameLayout::new(&scene.layout_parts(), Rect::new(0, 0, width, height));
        rendered(width, height, |frame| {
            frame.render_widget(
                &Screen {
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
        insta::assert_snapshot!(frame(sources.scene(), &CoverArt::Missing, (80, 24)));
    }

    #[test]
    fn the_cover_art_decides_whether_the_placeholder_is_painted() {
        let sources = SceneSources::new(model_with_tracks(3));
        let scene = Scene {
            pixel_path: PixelPath::Protocol,
            ..sources.scene()
        };
        let missing = frame(scene, &CoverArt::Missing, (80, 24));
        let image = frame(scene, &CoverArt::Image, (80, 24));
        assert!(missing.contains("No cover"), "got {missing}");
        assert!(!image.contains("No cover"), "got {image}");
    }

    #[test]
    fn a_short_terminal_paints_the_compact_card() {
        let sources = SceneSources::new(model_with_tracks(3));
        let text = frame(sources.scene(), &CoverArt::Missing, (80, 16));
        assert!(text.contains("No track"), "got {text}");
        assert!(text.contains("song00"), "got {text}");
    }

    #[test]
    fn a_terminal_below_the_minimum_shows_only_the_notice() {
        let sources = SceneSources::new(model_with_tracks(3));
        let text = frame(sources.scene(), &CoverArt::Missing, (40, 10));
        assert!(text.contains("Terminal too small."), "got {text}");
        assert!(!text.contains("song00"), "got {text}");
    }

    #[test]
    fn the_toast_is_painted_over_the_frame() {
        let mut model = model_with_tracks(3);
        model.workspace.toast = Some(Toast::info("Saved".to_string()));
        let sources = SceneSources::new(model);
        let text = frame(sources.scene(), &CoverArt::Missing, (80, 24));
        assert!(text.contains("Saved"), "got {text}");
    }
}
