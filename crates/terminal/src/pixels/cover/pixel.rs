use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use config::Animations;
use image::{DynamicImage, RgbaImage, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::{FontSize, picker::Picker, protocol::StatefulProtocol};
use widgets::{FrameLayout, Scene};

use crate::pixels::cover::{
    CoverArtOwner,
    CoverFade,
    CoverMotion,
    CoverWash,
    DecodedCover,
    crossfade::{CoverCrossfade, CrossfadeStage},
    protocol::cover_protocol,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverPlan {
    Reuse,
    RebuildSamePath,
    RebuildNewPath,
}

#[must_use]
pub(crate) fn plan_cover(
    decoded_path: &Path,
    painted: Option<(&Path, Rect)>,
    rect: Rect,
) -> CoverPlan {
    match painted {
        Some((path, painted_rect)) if path == decoded_path && painted_rect == rect => {
            CoverPlan::Reuse
        }
        Some((path, _)) if path == decoded_path => CoverPlan::RebuildSamePath,
        Some(_) | None => CoverPlan::RebuildNewPath,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlainSources<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
    pub(crate) decoded: Option<&'a DecodedCover>,
    pub(crate) fade: CoverFade,
    pub(crate) wash: CoverWash,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Transparency {
    Present,
    #[default]
    Absent,
}

#[must_use]
fn transparency(image: &RgbaImage) -> Transparency {
    if image.pixels().any(|pixel| pixel.0[3] < 255) {
        Transparency::Present
    } else {
        Transparency::Absent
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WashHold {
    Holding,
    #[default]
    Released,
}

struct PaintedCover {
    protocol: StatefulProtocol,
    path: PathBuf,
    rect: Rect,
}

#[derive(Debug, Clone, Copy)]
struct InstallPlain<'a> {
    rect: Rect,
    now: Duration,
    animations: Animations,
    decoded: Option<&'a DecodedCover>,
    fade: CoverFade,
}

struct PaintTarget {
    path: PathBuf,
    rect: Rect,
    now: Duration,
}

#[derive(Debug, Clone, Copy)]
struct AdvanceCrossfade {
    now: Duration,
    wash: CoverWash,
}

#[derive(Default)]
pub(crate) struct PlainCover {
    painted: Option<PaintedCover>,
    pixmap: Option<RgbaImage>,
    crossfade: CoverCrossfade,
    transparency: Transparency,
    wash_hold: WashHold,
}

impl fmt::Debug for PlainCover {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlainCover")
            .field(
                "painted",
                &self
                    .painted
                    .as_ref()
                    .map(|painted| (&painted.path, painted.rect)),
            )
            .finish()
    }
}

impl PlainCover {
    pub(crate) fn discard_protocol(&mut self) {
        self.painted = None;
        self.pixmap = None;
        self.crossfade = CoverCrossfade::default();
        self.transparency = Transparency::Absent;
        self.wash_hold = WashHold::Released;
    }

    pub(crate) fn refresh(
        &mut self,
        picker: &Picker,
        sources: PlainSources<'_>,
    ) -> CoverArtOwner {
        let PlainSources {
            scene,
            layout,
            decoded,
            fade,
            wash,
        } = sources;
        let Some(rect) = layout.cover else {
            self.discard_protocol();
            return CoverArtOwner::Missing;
        };
        let Some(decoded_path) = decoded.map(|decoded| decoded.path.as_path()) else {
            return CoverArtOwner::Missing;
        };
        let painted = self
            .painted
            .as_ref()
            .map(|painted| (painted.path.as_path(), painted.rect));
        match plan_cover(decoded_path, painted, rect) {
            CoverPlan::Reuse => {
                self.advance_crossfade(
                    picker,
                    AdvanceCrossfade {
                        now: scene.clock,
                        wash,
                    },
                );
            }
            CoverPlan::RebuildSamePath => {
                self.install(
                    picker,
                    InstallPlain {
                        rect,
                        now: scene.clock,
                        animations: scene.appearance.window.animations,
                        decoded,
                        fade: CoverFade::Withheld,
                    },
                );
            }
            CoverPlan::RebuildNewPath => {
                self.install(
                    picker,
                    InstallPlain {
                        rect,
                        now: scene.clock,
                        animations: scene.appearance.window.animations,
                        decoded,
                        fade,
                    },
                );
            }
        }
        CoverArtOwner::Image
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        self.painted.as_mut().map(|painted| &mut painted.protocol)
    }

    pub(crate) fn motion(&self, now: Duration) -> CoverMotion {
        match (self.crossfade.stage(now), self.wash_hold) {
            (CrossfadeStage::Running | CrossfadeStage::Over, _)
            | (CrossfadeStage::Idle, WashHold::Holding) => CoverMotion::Crossfading,
            (CrossfadeStage::Idle, WashHold::Released) => CoverMotion::Still,
        }
    }

    fn install(&mut self, picker: &Picker, input: InstallPlain<'_>) {
        let Some(decoded) = input.decoded else {
            return;
        };
        let outgoing = self.pixmap.take();
        if input.animations == Animations::On
            && input.fade == CoverFade::Allowed
            && let Some(outgoing) = outgoing
        {
            self.crossfade.begin(outgoing, input.now);
        }
        self.transparency = transparency(&decoded.image);
        self.pixmap = Some(decoded.image.clone());
        self.repaint(
            picker,
            PaintTarget {
                path: decoded.path.clone(),
                rect: input.rect,
                now: input.now,
            },
        );
    }

    fn advance_crossfade(&mut self, picker: &Picker, input: AdvanceCrossfade) {
        let AdvanceCrossfade { now, wash } = input;
        self.wash_hold = match (self.transparency, wash) {
            (Transparency::Present, CoverWash::Running { .. }) => WashHold::Holding,
            (Transparency::Present, CoverWash::Idle) | (Transparency::Absent, _) => {
                WashHold::Released
            }
        };
        match (self.crossfade.stage(now), self.wash_hold) {
            (CrossfadeStage::Idle, WashHold::Released) => {}
            (CrossfadeStage::Idle, WashHold::Holding)
            | (CrossfadeStage::Running, _) => self.repaint_current(picker, now),
            (CrossfadeStage::Over, _) => {
                self.crossfade.settle(now);
                self.repaint_current(picker, now);
            }
        }
    }

    fn repaint_current(&mut self, picker: &Picker, now: Duration) {
        let Some(painted) = self.painted.as_ref() else {
            return;
        };
        let target = PaintTarget {
            path: painted.path.clone(),
            rect: painted.rect,
            now,
        };
        self.repaint(picker, target);
    }

    fn repaint(&mut self, picker: &Picker, target: PaintTarget) {
        let Some(pixmap) = self.pixmap.as_ref() else {
            return;
        };
        let image = self
            .crossfade
            .crossfade_at(pixmap, target.now)
            .unwrap_or_else(|| pixmap.clone());
        let image = fit_to_rect(image, target.rect, picker.font_size());
        let protocol = cover_protocol(picker, DynamicImage::ImageRgba8(image));
        self.painted = Some(PaintedCover {
            protocol,
            path: target.path,
            rect: target.rect,
        });
    }
}

/// Resizes the plain cover's pixmap to exactly fill `rect` in pixels, so the
/// placed image never depends on the resize protocol's own fit heuristics
/// for a source resolution that may differ from the decoded cover's size.
#[must_use]
fn fit_to_rect(image: RgbaImage, rect: Rect, font_size: FontSize) -> RgbaImage {
    let width = u32::from(rect.width)
        .saturating_mul(u32::from(font_size.width))
        .max(1);
    let height = u32::from(rect.height)
        .saturating_mul(u32::from(font_size.height))
        .max(1);
    if image.width() == width && image.height() == height {
        return image;
    }
    image::imageops::resize(&image, width, height, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{Rgba, RgbaImage};
    use ratatui::layout::Rect;
    use ratatui_image::FontSize;
    use rstest::rstest;

    use crate::pixels::cover::pixel::{CoverPlan, fit_to_rect, plan_cover};

    struct PlanCase {
        decoded_path: PathBuf,
        painted: Option<(PathBuf, Rect)>,
        rect: Rect,
    }

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    #[rstest]
    #[case::a_compact_card(Rect::new(0, 0, 16, 8), FontSize { width: 9, height: 18 })]
    #[case::a_wide_terminal_card(Rect::new(2, 3, 24, 12), FontSize { width: 8, height: 16 })]
    #[case::a_tall_cell_font(Rect::new(0, 0, 30, 15), FontSize { width: 10, height: 20 })]
    fn a_plain_cover_is_fit_to_exactly_the_cover_squares_own_pixel_size(
        #[case] rect: Rect,
        #[case] font_size: FontSize,
    ) {
        let fitted = fit_to_rect(source_pixmap(), rect, font_size);
        assert_eq!(
            fitted.width(),
            u32::from(rect.width) * u32::from(font_size.width),
            "the fitted width must match the cover square converted to pixels"
        );
        assert_eq!(
            fitted.height(),
            u32::from(rect.height) * u32::from(font_size.height),
            "the fitted height must match the cover square converted to pixels"
        );
    }

    #[test]
    fn a_square_cover_cell_rect_stays_square_in_pixels() {
        let cell_aspect: u16 = 2;
        let font_size = FontSize {
            width: 9,
            height: 9 * cell_aspect,
        };
        let height_cells = 8u16;
        let width_cells = height_cells * cell_aspect;
        let rect = Rect::new(0, 0, width_cells, height_cells);

        let fitted = fit_to_rect(source_pixmap(), rect, font_size);
        assert_eq!(
            fitted.width(),
            fitted.height(),
            "a plain cover's cell rect built with cover_aspect 1.0 must render \
             as a square in pixels once fit to the target"
        );
    }

    fn rect() -> Rect {
        Rect::new(0, 0, 10, 10)
    }

    fn other_rect() -> Rect {
        Rect::new(0, 0, 12, 10)
    }

    #[rstest]
    #[case(
        PlanCase { decoded_path: PathBuf::from("a.jpg"), painted: None, rect: rect() },
        CoverPlan::RebuildNewPath
    )]
    #[case(
        PlanCase {
            decoded_path: PathBuf::from("a.jpg"),
            painted: Some((PathBuf::from("a.jpg"), rect())),
            rect: rect(),
        },
        CoverPlan::Reuse
    )]
    #[case(
        PlanCase {
            decoded_path: PathBuf::from("b.jpg"),
            painted: Some((PathBuf::from("a.jpg"), rect())),
            rect: rect(),
        },
        CoverPlan::RebuildNewPath
    )]
    #[case(
        PlanCase {
            decoded_path: PathBuf::from("a.jpg"),
            painted: Some((PathBuf::from("a.jpg"), rect())),
            rect: other_rect(),
        },
        CoverPlan::RebuildSamePath
    )]
    fn plan_cover_decides_reuse_or_rebuild(
        #[case] case: PlanCase,
        #[case] expected: CoverPlan,
    ) {
        let painted = case
            .painted
            .as_ref()
            .map(|(path, cell)| (path.as_path(), *cell));
        assert_eq!(plan_cover(&case.decoded_path, painted, case.rect), expected);
    }
}
