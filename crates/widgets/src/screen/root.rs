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
    playlist::{pane::PlaylistWidget, view::PlaylistView},
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
    scene: Scene<'a>,
    frame_layout: &'a FrameLayout<'a>,
    card_cover: &'a CardCover,
}

impl<'a> ScreenWidget<'a> {
    #[must_use]
    pub fn new(scene: Scene<'a>, frame_layout: &'a FrameLayout<'a>) -> Self {
        Self {
            scene,
            frame_layout,
            card_cover: &CardCover::Missing,
        }
    }

    #[must_use]
    pub fn card_cover(mut self, card_cover: &'a CardCover) -> Self {
        self.card_cover = card_cover;
        self
    }
}

impl Widget for &ScreenWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.scene.active_theme();
        let colors = theme.colors();
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.foreground),
            )
            .render(area, buffer);
        let layout = self.frame_layout;
        match layout.breakpoint {
            Breakpoint::TooSmall => {
                let breakpoints = self.scene.presentation.appearance.breakpoints;
                (&TooSmallWidget::new(
                    Size::new(breakpoints.min_width.0, breakpoints.min_height.0),
                    theme,
                ))
                    .render(layout.screen, buffer);
                return;
            }
            Breakpoint::Minimal => {
                (&MinimalScreenWidget::new(
                    CardView::from_scene(&self.scene),
                    theme,
                    layout.progress_bar_width,
                )
                .speed_chip(self.scene.settings.appearance_settings.speed_chip))
                    .render(layout.screen, buffer);
            }
            Breakpoint::Full => self.paint_card(buffer),
            Breakpoint::Compact => {
                let card = CompactCardWidget::new(
                    CardView::from_scene(&self.scene),
                    theme,
                    layout.progress_bar_width,
                )
                .speed_chip(self.scene.settings.appearance_settings.speed_chip);
                (&card).render(layout.header, buffer);
            }
        }
        self.paint_lists(buffer);
        self.paint_layers(buffer);
    }
}

impl ScreenWidget<'_> {
    fn paint_card(&self, buffer: &mut Buffer) {
        let Some(metrics) = self.frame_layout.card_metrics else {
            return;
        };
        let scene = self.scene;
        CardWidget::new(CardView::from_scene(&scene), scene.active_theme())
            .cell_aspect(scene.presentation.cell_aspect)
            .cover_sizing(scene.cover_sizing())
            .appearance_settings(scene.settings.appearance_settings)
            .card_cover(self.card_cover)
            .progress_bar_width(self.frame_layout.progress_bar_width)
            .remaining_label(&self.frame_layout.remaining_label)
            .paint(
                &metrics,
                Canvas {
                    area: self.frame_layout.header,
                    buffer,
                },
            );
    }

    fn paint_lists(&self, buffer: &mut Buffer) {
        let scene = self.scene;
        let theme = scene.active_theme();
        if let Some(areas) = self.frame_layout.playlist_areas {
            PlaylistWidget::new(PlaylistView::from_scene(&scene), theme)
                .paint(&areas, buffer);
        }
        if let Some(hints) = self.frame_layout.key_hints {
            (&KeyHintsWidget::new(KeyHintsView::from_scene(&scene), theme))
                .render(hints, buffer);
        }
    }

    fn paint_layers(&self, buffer: &mut Buffer) {
        let screen = self.frame_layout.screen;
        if let Some(areas) = self.frame_layout.overlay_areas {
            OverlayWidget::new(OverlayView::from_scene(&self.scene), self.frame_layout)
                .avoid(self.frame_layout.cover_exclusion(self.scene.cover_mode()))
                .paint(
                    areas,
                    Canvas {
                        area: screen,
                        buffer: &mut *buffer,
                    },
                );
        }
        if let Some((toast, areas)) =
            ToastWidget::from_scene(&self.scene).zip(self.frame_layout.toast)
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
    use std::time::Duration;

    use kernel::domain::{
        appearance::ProgressTime,
        geometry::Cells,
        player::Player,
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        toast::Toast,
    };
    use ratatui::layout::{Rect, Size};
    use rstest::rstest;

    use crate::{
        card::CardCover,
        primitive::glyphs,
        scene::{PixelPath, Scene},
        screen::{
            breakpoint::Breakpoint,
            frame_layout::FrameLayout,
            root::ScreenWidget,
        },
        test_support::{SceneSources, model_with_tracks, rendered, track},
    };

    fn frame(scene: Scene<'_>, card_cover: &CardCover, size: (u16, u16)) -> String {
        let (width, height) = size;
        let layout = FrameLayout::from_scene(&scene, Rect::new(0, 0, width, height));
        rendered(width, height, |frame| {
            frame.render_widget(
                &ScreenWidget::new(scene, &layout).card_cover(card_cover),
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
    fn the_card_cover_decides_whether_the_placeholder_is_painted() {
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

    #[test]
    fn the_painted_full_card_shows_the_layout_remaining_label() {
        let mut model = model_with_tracks(1);
        model.player = Player::Playing {
            track: track("song00"),
            playhead: Playhead::anchored(
                Duration::from_secs(30),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        };
        model.settings.appearance_settings.progress_time = ProgressTime::Remaining;
        let sources = SceneSources::new(model);
        let scene = sources.scene();
        let layout = FrameLayout::from_scene(&scene, Rect::new(0, 0, 80, 24));
        assert_eq!(layout.breakpoint, Breakpoint::Full);
        assert!(!layout.remaining_label.is_empty());
        let text = rendered(80, 24, |frame| {
            frame.render_widget(
                &ScreenWidget::new(scene, &layout).card_cover(&CardCover::Missing),
                frame.area(),
            );
        })
        .to_string();
        assert!(text.contains(&layout.remaining_label), "got {text}");
    }

    #[rstest]
    #[case::full_with_chip(
        Size::new(80, 24),
        ProgressTime::Remaining,
        Breakpoint::Full
    )]
    #[case::full_without_chip(
        Size::new(80, 24),
        ProgressTime::Elapsed,
        Breakpoint::Full
    )]
    #[case::compact(Size::new(80, 18), ProgressTime::Elapsed, Breakpoint::Compact)]
    #[case::minimal(Size::new(20, 5), ProgressTime::Elapsed, Breakpoint::Minimal)]
    fn the_painted_bar_is_as_wide_as_the_layout_says(
        #[case] size: Size,
        #[case] progress_time: ProgressTime,
        #[case] breakpoint: Breakpoint,
    ) {
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.appearance_mut().breakpoints.min_width = Cells(10);
        sources.appearance_mut().breakpoints.min_height = Cells(3);
        sources.model.settings.appearance_settings.progress_time = progress_time;
        let scene = sources.scene();
        let area = Rect::new(0, 0, size.width, size.height);
        let layout = FrameLayout::from_scene(&scene, area);
        assert_eq!(layout.breakpoint, breakpoint);
        let row = match layout.breakpoint {
            Breakpoint::Full => layout
                .card_metrics
                .map_or(0, |metrics| metrics.progress_row.y),
            Breakpoint::Compact => layout.header.y + 3,
            Breakpoint::Minimal | Breakpoint::TooSmall => layout.screen.y + 1,
        };
        let backend = rendered(size.width, size.height, |frame| {
            frame.render_widget(
                &ScreenWidget::new(scene, &layout).card_cover(&CardCover::Missing),
                frame.area(),
            );
        });
        let bar_glyphs = [
            glyphs::progress_line::FULL,
            glyphs::progress_line::PARTIAL,
            glyphs::progress_line::EMPTY,
        ];
        let bar_cells = (area.left()..area.right())
            .filter(|&x| bar_glyphs.contains(&backend.buffer()[(x, row)].symbol()))
            .count();
        assert_eq!(
            layout.progress_bar_width,
            Cells(u16::try_from(bar_cells).unwrap())
        );
    }
}
