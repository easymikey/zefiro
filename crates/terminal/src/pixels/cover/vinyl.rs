use std::{fmt, time::Duration};

use config::Animations;
use image::{DynamicImage, RgbaImage};
use raster::{
    ArtCacheState,
    DecodedArt,
    SleeveFace,
    VinylArtSource,
    VinylCache,
    VinylCacheKey,
    VinylColors,
    VinylImage,
    VinylLayout,
    VinylRequest,
    compose,
    dimension_f32,
    dimension_u32,
};
use ratatui::layout::Rect;
use ratatui_image::{FontSize, picker::Picker, protocol::StatefulProtocol};
use widgets::{Cells, FrameLayout, Pixels, Scene};

use crate::pixels::cover::{
    CoverArtOwner,
    CoverFade,
    CoverMotion,
    CoverWash,
    DecodedCover,
    crossfade::{CoverCrossfade, CrossfadeStage},
    protocol::cover_protocol,
    wash::{WashFrame, wash_frame},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct VinylSources<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
    pub(crate) decoded: Option<&'a DecodedCover>,
    pub(crate) fade: CoverFade,
    pub(crate) wash: CoverWash,
}

struct PaintedVinyl {
    protocol: StatefulProtocol,
    key: VinylCacheKey,
    rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VinylPaintKey<'a> {
    key: &'a VinylCacheKey,
    rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VinylPlan {
    Reuse,
    RebuildSameKey,
    RebuildThemeWash,
    RebuildNewKey,
}

#[must_use]
fn key_changed_only_by_theme(old: &VinylCacheKey, new: &VinylCacheKey) -> bool {
    old.theme_generation != new.theme_generation
        && old.config_generation == new.config_generation
        && old.path == new.path
        && old.face == new.face
        && old.size_px == new.size_px
}

#[must_use]
fn plan_vinyl(
    painted: Option<VinylPaintKey<'_>>,
    desired: VinylPaintKey<'_>,
) -> VinylPlan {
    match painted {
        Some(painted) if painted == desired => VinylPlan::Reuse,
        Some(painted)
            if painted.rect == desired.rect
                && key_changed_only_by_theme(painted.key, desired.key) =>
        {
            VinylPlan::RebuildThemeWash
        }
        Some(painted) if painted.key == desired.key => VinylPlan::RebuildSameKey,
        Some(_) | None => VinylPlan::RebuildNewKey,
    }
}

fn rebuild_kind(
    painted: Option<&PaintedVinyl>,
    key: &VinylCacheKey,
    rect: Rect,
) -> Option<RebuildKind> {
    let desired = VinylPaintKey { key, rect };
    let painted = painted.map(|painted| VinylPaintKey {
        key: &painted.key,
        rect: painted.rect,
    });
    RebuildKind::from_plan(plan_vinyl(painted, desired))
}

struct InstallVinyl {
    pixmap: RgbaImage,
    key: VinylCacheKey,
    rect: Rect,
    now: Duration,
    animations: Animations,
    fade: CoverFade,
}

#[derive(Debug, Clone, Copy)]
struct WashState {
    wash: CoverWash,
    cell_width_px: u16,
}

impl WashState {
    fn settled() -> Self {
        Self {
            wash: CoverWash::Idle,
            cell_width_px: 1,
        }
    }
}

struct BeginThemeWash {
    pixmap: RgbaImage,
    key: VinylCacheKey,
    rect: Rect,
    now: Duration,
    wash: WashState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebuildKind {
    ThemeWash,
    SameKey,
    NewKey,
}

impl RebuildKind {
    fn from_plan(plan: VinylPlan) -> Option<Self> {
        match plan {
            VinylPlan::Reuse => None,
            VinylPlan::RebuildThemeWash => Some(Self::ThemeWash),
            VinylPlan::RebuildSameKey => Some(Self::SameKey),
            VinylPlan::RebuildNewKey => Some(Self::NewKey),
        }
    }
}

struct RebuildInput {
    kind: RebuildKind,
    pixmap: RgbaImage,
    key: VinylCacheKey,
    rect: Rect,
    now: Duration,
    animations: Animations,
    fade: CoverFade,
    wash: WashState,
}

struct PaintTarget {
    key: VinylCacheKey,
    rect: Rect,
    now: Duration,
    wash: WashState,
}

#[derive(Debug, Clone, Copy)]
struct AdvanceTiming {
    now: Duration,
    wash: WashState,
}

#[derive(Default)]
pub(crate) struct VinylCover {
    cache: VinylCache,
    painted: Option<PaintedVinyl>,
    pixmap: Option<RgbaImage>,
    theme_wash: Option<RgbaImage>,
    crossfade: CoverCrossfade,
}

impl fmt::Debug for VinylCover {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VinylCover")
            .field(
                "painted",
                &self
                    .painted
                    .as_ref()
                    .map(|painted| (&painted.key, painted.rect)),
            )
            .finish()
    }
}

impl VinylCover {
    pub(crate) fn discard_protocol(&mut self) {
        self.painted = None;
        self.pixmap = None;
        self.theme_wash = None;
        self.crossfade = CoverCrossfade::default();
    }

    pub(crate) fn refresh(
        &mut self,
        picker: &Picker,
        sources: VinylSources<'_>,
    ) -> CoverArtOwner {
        let VinylSources {
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
        let font_size = picker.font_size();
        let size_px = vinyl_size_px(rect, font_size);
        let art = vinyl_art_source(&self.cache, decoded, size_px);
        let request = VinylRequest {
            cache: &mut self.cache,
            colors: VinylColors::from(scene.theme),
            art,
            size_px,
            face: SleeveFace::Art,
            config_generation: scene.model.config_generation,
            theme_generation: scene.model.theme_generation,
        };
        let VinylImage::Ready { pixmap, key } = compose(request) else {
            self.discard_protocol();
            return CoverArtOwner::Missing;
        };
        let wash_state = WashState {
            wash,
            cell_width_px: font_size.width,
        };
        let Some(kind) = rebuild_kind(self.painted.as_ref(), &key, rect) else {
            self.advance(
                picker,
                AdvanceTiming {
                    now: scene.clock,
                    wash: wash_state,
                },
            );
            return CoverArtOwner::Image;
        };
        let pixmap = pixmap.clone();
        self.rebuild(
            picker,
            RebuildInput {
                kind,
                pixmap,
                key,
                rect,
                now: scene.clock,
                animations: scene.appearance.window.animations,
                fade,
                wash: wash_state,
            },
        );
        CoverArtOwner::Image
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        self.painted.as_mut().map(|painted| &mut painted.protocol)
    }

    pub(crate) fn motion(&self, now: Duration) -> CoverMotion {
        if self.theme_wash.is_some() {
            return CoverMotion::Crossfading;
        }
        match self.crossfade.stage(now) {
            CrossfadeStage::Running | CrossfadeStage::Over => CoverMotion::Crossfading,
            CrossfadeStage::Idle => CoverMotion::Still,
        }
    }

    fn rebuild(&mut self, picker: &Picker, input: RebuildInput) {
        self.theme_wash = None;
        match input.kind {
            RebuildKind::ThemeWash => self.begin_theme_wash(
                picker,
                BeginThemeWash {
                    pixmap: input.pixmap,
                    key: input.key,
                    rect: input.rect,
                    now: input.now,
                    wash: input.wash,
                },
            ),
            RebuildKind::SameKey => self.install(
                picker,
                InstallVinyl {
                    pixmap: input.pixmap,
                    key: input.key,
                    rect: input.rect,
                    now: input.now,
                    animations: input.animations,
                    fade: CoverFade::Withheld,
                },
            ),
            RebuildKind::NewKey => self.install(
                picker,
                InstallVinyl {
                    pixmap: input.pixmap,
                    key: input.key,
                    rect: input.rect,
                    now: input.now,
                    animations: input.animations,
                    fade: input.fade,
                },
            ),
        }
    }

    fn install(&mut self, picker: &Picker, input: InstallVinyl) {
        let outgoing = self.pixmap.take();
        if input.animations == Animations::On
            && input.fade == CoverFade::Allowed
            && let Some(outgoing) = outgoing
        {
            self.crossfade.begin(outgoing, input.now);
        }
        self.pixmap = Some(input.pixmap);
        self.repaint(
            picker,
            PaintTarget {
                key: input.key,
                rect: input.rect,
                now: input.now,
                wash: WashState::settled(),
            },
        );
    }

    fn begin_theme_wash(&mut self, picker: &Picker, input: BeginThemeWash) {
        let outgoing = self.pixmap.take();
        self.pixmap = Some(input.pixmap);
        if let CoverWash::Running { .. } = input.wash.wash {
            self.theme_wash = outgoing;
        }
        self.repaint(
            picker,
            PaintTarget {
                key: input.key,
                rect: input.rect,
                now: input.now,
                wash: input.wash,
            },
        );
    }

    fn advance(&mut self, picker: &Picker, timing: AdvanceTiming) {
        match (self.theme_wash.is_some(), timing.wash.wash) {
            (true, CoverWash::Running { .. }) => self.repaint_current(picker, timing),
            (true, CoverWash::Idle) => {
                self.theme_wash = None;
                self.repaint_current(
                    picker,
                    AdvanceTiming {
                        now: timing.now,
                        wash: WashState::settled(),
                    },
                );
            }
            (false, _) => self.advance_crossfade(picker, timing.now),
        }
    }

    fn advance_crossfade(&mut self, picker: &Picker, now: Duration) {
        let settled = AdvanceTiming {
            now,
            wash: WashState::settled(),
        };
        match self.crossfade.stage(now) {
            CrossfadeStage::Idle => {}
            CrossfadeStage::Running => self.repaint_current(picker, settled),
            CrossfadeStage::Over => {
                self.crossfade.settle(now);
                self.repaint_current(picker, settled);
            }
        }
    }

    fn repaint_current(&mut self, picker: &Picker, timing: AdvanceTiming) {
        let Some(painted) = self.painted.as_ref() else {
            return;
        };
        let target = PaintTarget {
            key: painted.key.clone(),
            rect: painted.rect,
            now: timing.now,
            wash: timing.wash,
        };
        self.repaint(picker, target);
    }

    fn repaint(&mut self, picker: &Picker, target: PaintTarget) {
        let Some(pixmap) = self.pixmap.as_ref() else {
            return;
        };
        let image = match (self.theme_wash.as_ref(), target.wash.wash) {
            (
                Some(old),
                CoverWash::Running {
                    progress,
                    screen_width,
                },
            ) => wash_frame(WashFrame {
                old,
                new: pixmap,
                rect: target.rect,
                cell_width_px: target.wash.cell_width_px,
                progress,
                screen_width,
            }),
            _ => self
                .crossfade
                .crossfade_at(pixmap, target.now)
                .unwrap_or_else(|| pixmap.clone()),
        };
        let protocol = cover_protocol(picker, DynamicImage::ImageRgba8(image));
        self.painted = Some(PaintedVinyl {
            protocol,
            key: target.key,
            rect: target.rect,
        });
    }
}

fn vinyl_size_px(rect: Rect, font_size: FontSize) -> u32 {
    Cells::from(rect.height)
        .to_pixels(Pixels::from(u32::from(font_size.height)))
        .get()
}

fn sleeve_inset_side_px(size_px: u32) -> u32 {
    let layout = VinylLayout::default();
    let size = dimension_f32(size_px.max(1));
    let pad = layout.sleeve_padding * size;
    dimension_u32((size - pad * 2.0).round()).max(1)
}

fn vinyl_art_source(
    cache: &VinylCache,
    decoded: Option<&DecodedCover>,
    size_px: u32,
) -> VinylArtSource {
    let Some(cover) = decoded else {
        return VinylArtSource::default();
    };
    let state = cache.art_cache_state(Some(&cover.path), size_px);
    if state == ArtCacheState::Cached {
        return VinylArtSource {
            path: Some(cover.path.clone()),
            decoded: None,
        };
    }
    VinylArtSource {
        path: Some(cover.path.clone()),
        decoded: Some(DecodedArt {
            side_px: sleeve_inset_side_px(size_px),
            image: Some(DynamicImage::ImageRgba8(cover.image.clone())),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{Rgba, RgbaImage};
    use kernel::domain::Revision;
    use raster::{
        SleeveFace,
        VinylCache,
        VinylCacheKey,
        VinylColors,
        VinylImage,
        VinylRequest,
        compose,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::pixels::cover::{
        DecodedCover,
        vinyl::{
            VinylPaintKey,
            VinylPlan,
            plan_vinyl,
            sleeve_inset_side_px,
            vinyl_art_source,
        },
    };

    fn key_for(path: &str) -> VinylCacheKey {
        VinylCacheKey {
            config_generation: Revision::default(),
            theme_generation: Revision::default(),
            path: Some(PathBuf::from(path)),
            face: SleeveFace::Art,
            size_px: 128,
        }
    }

    fn key_with_theme(path: &str, theme_generation: Revision) -> VinylCacheKey {
        VinylCacheKey {
            theme_generation,
            ..key_for(path)
        }
    }

    fn rect() -> Rect {
        Rect::new(0, 0, 10, 10)
    }

    struct PlanCase {
        painted: Option<(VinylCacheKey, Rect)>,
        desired: (VinylCacheKey, Rect),
    }

    #[rstest]
    #[case::same_track_and_rect_twice(
        PlanCase {
            painted: Some((key_for("a.flac"), rect())),
            desired: (key_for("a.flac"), rect()),
        },
        VinylPlan::Reuse
    )]
    #[case::a_different_key(
        PlanCase {
            painted: Some((key_for("a.flac"), rect())),
            desired: (key_for("b.flac"), rect()),
        },
        VinylPlan::RebuildNewKey
    )]
    #[case::a_different_rect(
        PlanCase {
            painted: Some((key_for("a.flac"), rect())),
            desired: (key_for("a.flac"), Rect::new(0, 0, 12, 10)),
        },
        VinylPlan::RebuildSameKey
    )]
    #[case::nothing_installed(
        PlanCase {
            painted: None,
            desired: (key_for("a.flac"), rect()),
        },
        VinylPlan::RebuildNewKey
    )]
    #[case::only_the_theme_generation_moved(
        PlanCase {
            painted: Some((key_for("a.flac"), rect())),
            desired: (key_with_theme("a.flac", Revision::default().next()), rect()),
        },
        VinylPlan::RebuildThemeWash
    )]
    fn plan_vinyl_decides_reuse_or_rebuild(
        #[case] case: PlanCase,
        #[case] expected: VinylPlan,
    ) {
        let painted = case
            .painted
            .as_ref()
            .map(|(installed_key, installed_rect)| VinylPaintKey {
                key: installed_key,
                rect: *installed_rect,
            });
        let (desired_key, desired_rect) = &case.desired;
        let desired = VinylPaintKey {
            key: desired_key,
            rect: *desired_rect,
        };
        assert_eq!(plan_vinyl(painted, desired), expected);
    }

    #[test]
    fn sleeve_inset_side_px_is_smaller_than_the_full_canvas() {
        assert!(sleeve_inset_side_px(128) < 128);
        assert!(sleeve_inset_side_px(128) > 0);
    }

    #[test]
    fn a_second_paint_of_the_same_track_builds_no_new_art_source() {
        let mut cache = VinylCache::default();
        let cover = DecodedCover {
            path: PathBuf::from("a.flac"),
            image: RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255])),
        };
        let size_px = 96;

        let first = vinyl_art_source(&cache, Some(&cover), size_px);
        assert!(
            first.decoded.is_some(),
            "an empty cache must build the art source"
        );

        let request = VinylRequest {
            cache: &mut cache,
            colors: VinylColors::default(),
            art: first,
            size_px,
            face: SleeveFace::Art,
            config_generation: Revision::default(),
            theme_generation: Revision::default(),
        };
        assert!(matches!(compose(request), VinylImage::Ready { .. }));

        let second = vinyl_art_source(&cache, Some(&cover), size_px);
        assert!(
            second.decoded.is_none(),
            "a cached key must not rebuild the art source"
        );
    }
}
