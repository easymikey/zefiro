mod cover;

use std::time::Duration;

use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};
use widgets::{FrameLayout, OverlayAreas};

use crate::pixels::cover::CoverPixels;
pub use crate::pixels::cover::{
    CoverArtOwner,
    CoverFade,
    CoverMotion,
    CoverSources,
    CoverWash,
    DecodedCover,
};

#[derive(Debug)]
pub struct Pixels {
    picker: Picker,
    cover: CoverPixels,
}

impl Pixels {
    #[must_use]
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            cover: CoverPixels::default(),
        }
    }

    pub fn adopt(&mut self, picker: Picker) {
        self.picker = picker;
        self.cover.discard_protocol();
    }

    pub fn decoded_cover(&mut self, cover: DecodedCover) {
        self.cover.accept(cover);
    }

    pub fn refresh(&mut self, sources: CoverSources<'_>) -> CoverArtOwner {
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
    let overlay = layout.overlay.map(OverlayAreas::painted);
    let toast = layout.toast.map(|toast| toast.painted);
    [overlay, toast]
        .into_iter()
        .flatten()
        .any(|painted| !painted.intersection(rect).is_empty())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use config::{AppearanceFile, Hex, ThemeColors};
    use image::{Rgba, RgbaImage};
    use kernel::{
        domain::{AudioFormat, KeymapOverrides, Model, Player, Preload, Tags, Track},
        update::keymap::Bindings,
    };
    use ratatui::layout::Rect;
    use ratatui_image::picker::Picker;
    use widgets::{
        AnimationTimings,
        Breakpoint,
        CellAspect,
        ColorDepth,
        Colors,
        FrameLayout,
        PixelPath,
        SPECTRUM_BANDS,
        Scene,
        Spectrum,
        Theme,
        ToastAreas,
    };

    use crate::pixels::{
        CoverFade,
        CoverMotion,
        CoverSources,
        CoverWash,
        DecodedCover,
        Pixels,
        hidden_by_overlay,
    };

    fn theme() -> Theme {
        let colors = ThemeColors {
            background: Hex([0x10, 0x10, 0x10]),
            foreground: Hex([0x80, 0x80, 0x80]),
            bright_foreground: Hex([0xe0, 0xe0, 0xe0]),
            accent: Hex([0xff, 0, 0]),
            green: Hex([0, 0xff, 0]),
            yellow: Hex([0xff, 0xff, 0]),
            red: Hex([0xff, 0, 0]),
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
                at: Duration::from_secs(position_secs),
                preload: Preload::None,
            },
            ..Model::default()
        }
    }

    struct Fixture {
        model: Model,
        theme: Theme,
        appearance: AppearanceFile,
        bindings: Bindings,
        spectrum: Spectrum,
    }

    impl Fixture {
        fn playing() -> Self {
            Self {
                model: playing(245, 30),
                theme: theme(),
                appearance: AppearanceFile::default(),
                bindings: Bindings::new(&KeymapOverrides::default()),
                spectrum: [0.0; SPECTRUM_BANDS],
            }
        }

        fn scene(&self) -> Scene<'_> {
            Scene {
                model: &self.model,
                theme: &self.theme,
                color_depth: ColorDepth::TrueColor,
                appearance: &self.appearance,
                bindings: self.bindings.as_slice(),
                spectrum: &self.spectrum,
                pixel_path: PixelPath::Protocol,
                cell_aspect: CellAspect::default(),
                clock: Duration::ZERO,
                now_unix: 0,
                music_dir: "/music",
                sleep_left: None,
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
            image: RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255])),
        }
    }

    fn crossfade_duration() -> Duration {
        Duration::from_millis(u64::from(AnimationTimings::default().cover_crossfade.0))
    }

    #[test]
    fn a_vinyl_rebuild_with_permission_on_a_new_path_crossfades_then_settles() {
        let fixture = Fixture::playing();
        let mut pixels = Pixels::new(Picker::halfblocks());
        let layout = layout_with_cover(Rect::new(0, 0, 10, 10));
        pixels.decoded_cover(decoded_cover("a.jpg"));
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
        pixels.decoded_cover(decoded_cover("b.jpg"));
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
        assert_eq!(
            pixels.cover_motion(Duration::ZERO),
            CoverMotion::Crossfading
        );
        assert_eq!(
            pixels.cover_motion(crossfade_duration()),
            CoverMotion::Crossfading
        );
        pixels.refresh(CoverSources {
            scene: Scene {
                clock: crossfade_duration(),
                ..fixture.scene()
            },
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
        assert_eq!(
            pixels.cover_motion(crossfade_duration()),
            CoverMotion::Still
        );
    }

    #[test]
    fn a_vinyl_rebuild_without_permission_never_crossfades() {
        let fixture = Fixture::playing();
        let mut pixels = Pixels::new(Picker::halfblocks());
        let layout = layout_with_cover(Rect::new(0, 0, 10, 10));
        pixels.decoded_cover(decoded_cover("a.jpg"));
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
        pixels.decoded_cover(decoded_cover("b.jpg"));
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        });
        assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
    }

    #[test]
    fn a_vinyl_rebuild_for_a_new_rect_on_the_same_path_never_crossfades() {
        let fixture = Fixture::playing();
        let mut pixels = Pixels::new(Picker::halfblocks());
        pixels.decoded_cover(decoded_cover("a.jpg"));
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout: layout_with_cover(Rect::new(0, 0, 10, 10)),
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
        pixels.refresh(CoverSources {
            scene: fixture.scene(),
            layout: layout_with_cover(Rect::new(0, 0, 12, 10)),
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        });
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
