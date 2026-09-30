mod cover;

use std::time::Duration;

use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};
use widgets::{FrameLayout, OverlayAreas};

use crate::pixels::cover::CoverPixels;
pub use crate::pixels::cover::{
    CoverFade,
    CoverKey,
    CoverLook,
    CoverMoment,
    CoverMotion,
    CoverParts,
    CoverPlacement,
    CoverWash,
    DecodedCover,
    OwnedCoverArt,
};

#[derive(Debug)]
pub struct CoverRenderer {
    picker: Picker,
    cover: CoverPixels,
}

impl CoverRenderer {
    #[must_use]
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            cover: CoverPixels::default(),
        }
    }

    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = picker;
        self.cover.discard_protocol();
    }

    pub fn set_cover(&mut self, cover: DecodedCover) {
        self.cover.set_cover(cover);
    }

    pub fn refresh(&mut self, sources: CoverParts<'_>) -> OwnedCoverArt {
        self.cover.refresh(&self.picker, sources)
    }

    #[must_use]
    pub fn cover_motion(&self, now: Duration) -> CoverMotion {
        self.cover.motion(now)
    }

    pub fn place(&mut self, buffer: &mut Buffer, layout: &FrameLayout) {
        place_protocol(
            buffer,
            Placement {
                rect: layout.cover,
                layout,
                protocol: self.cover.protocol_mut(),
            },
        );
    }
}

struct Placement<'a> {
    rect: Option<Rect>,
    layout: &'a FrameLayout,
    protocol: Option<&'a mut StatefulProtocol>,
}

fn place_protocol(buffer: &mut Buffer, placement: Placement<'_>) {
    let Placement {
        rect,
        layout,
        protocol,
    } = placement;
    let Some(rect) = rect else {
        return;
    };
    if hidden_by_overlay(rect, layout) {
        return;
    }
    let Some(protocol) = protocol else {
        return;
    };
    StatefulWidget::render(StatefulImage::default(), rect, buffer, protocol);
}

fn hidden_by_overlay(rect: Rect, layout: &FrameLayout) -> bool {
    let overlay = layout.overlay.map(OverlayAreas::outer);
    let toast = layout.toast.map(|toast| toast.painted);
    [overlay, toast]
        .into_iter()
        .flatten()
        .any(|painted| !painted.intersection(rect).is_empty())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use config::{AppearanceFile, Rgb, ThemeColors};
    use image::{Rgba, RgbaImage};
    use kernel::{
        Moment,
        domain::{AudioFormat, Model, Player, Playhead, Preload, Speed, Tags, Track},
    };
    use raster::{VinylColors, color_overrides};
    use ratatui::layout::Rect;
    use ratatui_image::picker::Picker;
    use widgets::{
        ActiveTheme,
        AnimationTimings,
        Breakpoint,
        ColorDepth,
        Colors,
        FrameLayout,
        MilkdropColors,
        Playing,
        SPECTRUM_BANDS,
        Spectrum,
        Theme,
        ToastAreas,
    };

    use crate::pixels::{
        CoverFade,
        CoverKey,
        CoverLook,
        CoverMoment,
        CoverMotion,
        CoverParts,
        CoverPlacement,
        CoverRenderer,
        CoverWash,
        DecodedCover,
        hidden_by_overlay,
    };

    fn theme() -> Theme {
        let colors = ThemeColors {
            background: Rgb([0x10, 0x10, 0x10]),
            foreground: Rgb([0x80, 0x80, 0x80]),
            bright_foreground: Rgb([0xe0, 0xe0, 0xe0]),
            accent: Rgb([0xff, 0, 0]),
            green: Rgb([0, 0xff, 0]),
            yellow: Rgb([0xff, 0xff, 0]),
            red: Rgb([0xff, 0, 0]),
            window_background: None,
        };
        Theme {
            name: "test".to_string(),
            colors: Colors::derive(&colors),
            scanning_label: String::new(),
        }
    }

    fn playing(duration_secs: u64, position_secs: u64) -> Model {
        let track = Arc::new(
            Track::builder()
                .path("song.mp3")
                .duration(Duration::from_secs(duration_secs))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        );
        Model {
            player: Player::Playing {
                track,
                head: Playhead::anchored(
                    Duration::from_secs(position_secs),
                    Moment::default(),
                    Speed::default(),
                ),
                preload: Preload::None,
            },
            ..Model::default()
        }
    }

    struct Fixture {
        model: Model,
        theme: Theme,
        appearance: AppearanceFile,
        spectrum: Spectrum,
    }

    impl Fixture {
        fn playing() -> Self {
            Self {
                model: playing(245, 30),
                theme: theme(),
                appearance: AppearanceFile::default(),
                spectrum: [0.0; SPECTRUM_BANDS],
            }
        }

        fn sources(&self, layout: FrameLayout, fade: CoverFade) -> CoverParts<'_> {
            CoverParts {
                key: CoverKey {
                    config_generation: self.model.revisions.config,
                    theme_generation: self.model.revisions.theme,
                },
                look: CoverLook {
                    style: self.appearance.cover.style,
                    animations: self.appearance.window.animations,
                    vinyl: VinylColors::from(&self.theme),
                    milkdrop: MilkdropColors::from_theme(
                        &ActiveTheme::new(&self.theme, ColorDepth::TrueColor)
                            .with_bars(color_overrides(&self.appearance.progress)),
                    ),
                },
                moment: CoverMoment {
                    clock: Duration::ZERO,
                    playing: if self.model.player.is_playing() {
                        Playing::Yes
                    } else {
                        Playing::No
                    },
                    track: self.model.player.current().map(|track| track.path()),
                    bands: &self.spectrum,
                },
                placement: CoverPlacement {
                    layout,
                    fade,
                    wash: CoverWash::Idle,
                },
            }
        }
    }

    fn layout_with_cover(cover: Rect) -> FrameLayout {
        FrameLayout {
            screen: Rect::default(),
            breakpoint: Breakpoint::Full,
            content: Rect::default(),
            header: Rect::default(),
            card: None,
            cover: Some(cover),
            playlist_pane: Rect::default(),
            playlist: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay: None,
            toast: None,
        }
    }

    fn decoded_cover(path: &str) -> DecodedCover {
        DecodedCover {
            path: PathBuf::from(path),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))),
        }
    }

    fn crossfade_duration() -> Duration {
        Duration::from_millis(u64::from(AnimationTimings::default().cover_crossfade.0))
    }

    #[test]
    fn a_vinyl_rebuild_with_permission_on_a_new_path_crossfades_then_settles() {
        let fixture = Fixture::playing();
        let mut pixels = CoverRenderer::new(Picker::halfblocks());
        let layout = layout_with_cover(Rect::new(0, 0, 10, 10));
        pixels.set_cover(decoded_cover("a.jpg"));
        pixels.refresh(fixture.sources(layout, CoverFade::Allowed));
        pixels.set_cover(decoded_cover("b.jpg"));
        pixels.refresh(fixture.sources(layout, CoverFade::Allowed));
        assert_eq!(
            pixels.cover_motion(Duration::ZERO),
            CoverMotion::Crossfading
        );
        assert_eq!(
            pixels.cover_motion(crossfade_duration()),
            CoverMotion::Crossfading
        );
        let mut settled = fixture.sources(layout, CoverFade::Allowed);
        settled.moment.clock = crossfade_duration();
        pixels.refresh(settled);
        assert_eq!(
            pixels.cover_motion(crossfade_duration()),
            CoverMotion::Still
        );
    }

    #[test]
    fn a_vinyl_rebuild_without_permission_never_crossfades() {
        let fixture = Fixture::playing();
        let mut pixels = CoverRenderer::new(Picker::halfblocks());
        let layout = layout_with_cover(Rect::new(0, 0, 10, 10));
        pixels.set_cover(decoded_cover("a.jpg"));
        pixels.refresh(fixture.sources(layout, CoverFade::Allowed));
        pixels.set_cover(decoded_cover("b.jpg"));
        pixels.refresh(fixture.sources(layout, CoverFade::Withheld));
        assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
    }

    #[test]
    fn a_vinyl_rebuild_for_a_new_rect_on_the_same_path_never_crossfades() {
        let fixture = Fixture::playing();
        let mut pixels = CoverRenderer::new(Picker::halfblocks());
        pixels.set_cover(decoded_cover("a.jpg"));
        pixels.refresh(fixture.sources(
            layout_with_cover(Rect::new(0, 0, 10, 10)),
            CoverFade::Allowed,
        ));
        pixels.refresh(fixture.sources(
            layout_with_cover(Rect::new(0, 0, 12, 10)),
            CoverFade::Allowed,
        ));
        assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
    }

    #[test]
    fn nothing_is_hidden_without_an_overlay_or_a_toast() {
        let rect = Rect::new(0, 0, 20, 1);
        let layout = layout_with_cover(rect);
        assert!(!hidden_by_overlay(rect, &layout));
    }

    #[test]
    fn a_toast_that_overlaps_the_rect_hides_it() {
        let rect = Rect::new(0, 0, 20, 1);
        let mut layout = layout_with_cover(rect);
        layout.toast = Some(ToastAreas {
            outer: Rect::new(10, 0, 10, 1),
            painted: Rect::new(10, 0, 10, 1),
        });
        assert!(hidden_by_overlay(rect, &layout));
    }

    #[test]
    fn a_toast_away_from_the_rect_leaves_it_visible() {
        let rect = Rect::new(0, 0, 20, 1);
        let mut layout = layout_with_cover(rect);
        layout.toast = Some(ToastAreas {
            outer: Rect::new(0, 5, 10, 1),
            painted: Rect::new(0, 5, 10, 1),
        });
        assert!(!hidden_by_overlay(rect, &layout));
    }
}
