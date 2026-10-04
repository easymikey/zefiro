use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use image::RgbaImage;
use kernel::domain::appearance::Animations;
use ratatui::layout::Rect;

use crate::{
    card::CardCover,
    pixels::{
        cover::{
            CoverImage,
            CoverMotion,
            CoverRefresh,
            CoverWash,
            CrossfadePermit,
            crossfade::{CoverCrossfade, CrossfadeStage, blend_by_column},
            pixmap::{
                CellPixels,
                compose_vinyl,
                fit_to_rect,
                plain_pixmap,
                translucent,
                vinyl_key,
                vinyl_size_px,
            },
            wash::column_reveal,
        },
        vinyl::{VinylCache, VinylCacheKey},
    },
    scene::Scene,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    Plain(PathBuf),
    Vinyl(VinylCacheKey),
}

impl Identity {
    fn changed_only_by_theme(&self, desired: &Self) -> bool {
        match (self, desired) {
            (Self::Vinyl(old), Self::Vinyl(new)) => {
                old.theme_revision != new.theme_revision
                    && old.config_revision == new.config_revision
                    && old.path == new.path
                    && old.size_px == new.size_px
            }
            (Self::Plain(_), _) | (Self::Vinyl(_), Self::Plain(_)) => false,
        }
    }
}

pub(crate) struct BuiltPixmap {
    pub pixmap: Arc<RgbaImage>,
    pub identity: Identity,
}

#[derive(Debug)]
pub enum PixmapSource {
    Plain,
    Vinyl(Box<VinylCache>),
}

impl PixmapSource {
    fn keeps_outgoing_during_wash(&self, pixmap: &RgbaImage) -> bool {
        match self {
            Self::Plain => translucent(pixmap),
            Self::Vinyl(_) => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaintPlan {
    Reuse,
    SameContent,
    ThemeWash,
    NewContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed<'a> {
    identity: &'a Identity,
    rect: Rect,
}

#[must_use]
fn plan_paint(painted: Option<Placed<'_>>, desired: Placed<'_>) -> PaintPlan {
    match painted {
        Some(painted) if painted == desired => PaintPlan::Reuse,
        Some(painted)
            if painted.rect == desired.rect
                && painted.identity.changed_only_by_theme(desired.identity) =>
        {
            PaintPlan::ThemeWash
        }
        Some(painted) if painted.identity == desired.identity => PaintPlan::SameContent,
        Some(_) | None => PaintPlan::NewContent,
    }
}

#[derive(Debug, Clone, Copy)]
struct Tick {
    now: Duration,
    wash: CoverWash,
}

struct Painted {
    identity: Identity,
    rect: Rect,
}

impl Painted {
    fn placed(&self) -> Placed<'_> {
        Placed {
            identity: &self.identity,
            rect: self.rect,
        }
    }
}

struct PaintTarget {
    identity: Identity,
    rect: Rect,
    tick: Tick,
}

struct RebuildParts {
    plan: PaintPlan,
    built: BuiltPixmap,
    rect: Rect,
    crossfade: CrossfadePermit,
    tick: Tick,
}

#[derive(Debug)]
pub enum CoverFrame {
    Keep,
    Repaint(RgbaImage),
    Forget,
}

#[derive(Debug)]
pub struct CoverUpdate {
    pub art: CardCover,
    pub frame: CoverFrame,
}

pub struct CoverLifecycle {
    source: PixmapSource,
    cell: CellPixels,
    decoded: Option<CoverImage>,
    painted: Option<Painted>,
    pixmap: Option<Arc<RgbaImage>>,
    crossfade: CoverCrossfade,
    wash_outgoing: Option<Arc<RgbaImage>>,
}

impl fmt::Debug for CoverLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoverLifecycle")
            .field(
                "painted",
                &self
                    .painted
                    .as_ref()
                    .map(|painted| (&painted.identity, painted.rect)),
            )
            .finish()
    }
}

impl CoverLifecycle {
    #[must_use]
    pub fn new(source: PixmapSource, cell: CellPixels) -> Self {
        Self {
            source,
            cell,
            decoded: None,
            painted: None,
            pixmap: None,
            crossfade: CoverCrossfade::default(),
            wash_outgoing: None,
        }
    }

    pub fn set_cell(&mut self, cell: CellPixels) {
        self.cell = cell;
        self.forget_painted();
    }

    pub fn set_cover(&mut self, decoded: CoverImage) {
        self.decoded = Some(decoded);
    }

    fn forget_painted(&mut self) {
        self.painted = None;
        self.pixmap = None;
        self.wash_outgoing = None;
        self.crossfade = CoverCrossfade::default();
    }

    pub fn refresh(&mut self, scene: &Scene<'_>, parts: CoverRefresh) -> CoverUpdate {
        let CoverRefresh {
            cover,
            crossfade,
            wash,
        } = parts;
        let Some(rect) = cover else {
            return self.forget();
        };
        let Some(built) = self.build(scene, rect) else {
            return self.forget();
        };
        let tick = Tick {
            now: scene.clock,
            wash,
        };
        let desired = Placed {
            identity: &built.identity,
            rect,
        };
        let plan = plan_paint(self.painted.as_ref().map(Painted::placed), desired);
        if plan == PaintPlan::Reuse {
            return CoverUpdate {
                art: CardCover::Image,
                frame: self.advance(tick),
            };
        }
        let crossfade = if scene.appearance_settings().animations == Animations::On
            && plan == PaintPlan::NewContent
        {
            crossfade
        } else {
            CrossfadePermit::Withheld
        };
        let frame = self.rebuild(RebuildParts {
            plan,
            built,
            rect,
            crossfade,
            tick,
        });
        CoverUpdate {
            art: CardCover::Image,
            frame,
        }
    }

    #[must_use]
    pub fn motion(&self, now: Duration) -> CoverMotion {
        match (self.crossfade.stage(now), self.wash_outgoing.is_some()) {
            (CrossfadeStage::Idle, false) => CoverMotion::Still,
            (CrossfadeStage::Idle, true)
            | (CrossfadeStage::Running | CrossfadeStage::Over, _) => {
                CoverMotion::Animating
            }
        }
    }

    fn forget(&mut self) -> CoverUpdate {
        self.forget_painted();
        CoverUpdate {
            art: CardCover::Missing,
            frame: CoverFrame::Forget,
        }
    }

    fn build(&mut self, scene: &Scene<'_>, rect: Rect) -> Option<BuiltPixmap> {
        let decoded = self.decoded.as_ref();
        match &mut self.source {
            PixmapSource::Plain => plain_pixmap(decoded),
            PixmapSource::Vinyl(cache) => {
                let size_px = vinyl_size_px(rect, self.cell);
                let key = vinyl_key(scene, decoded, size_px);
                Some(compose_vinyl(cache, key, decoded))
            }
        }
    }

    fn rebuild(&mut self, parts: RebuildParts) -> CoverFrame {
        let RebuildParts {
            plan,
            built,
            rect,
            crossfade,
            tick,
        } = parts;
        let outgoing = self.pixmap.replace(built.pixmap);
        self.wash_outgoing = None;
        let shown = if plan == PaintPlan::ThemeWash {
            if matches!(tick.wash, CoverWash::Running { .. }) {
                self.wash_outgoing = outgoing;
            }
            tick
        } else {
            if crossfade == CrossfadePermit::Allowed
                && let Some(outgoing) = outgoing
            {
                self.crossfade.begin(outgoing, tick.now);
            }
            Tick {
                wash: CoverWash::Idle,
                ..tick
            }
        };
        let frame = self.paint(PaintTarget {
            identity: built.identity,
            rect,
            tick: shown,
        });
        self.hold_wash_outgoing(tick.wash);
        frame
    }

    fn hold_wash_outgoing(&mut self, wash: CoverWash) {
        if matches!(wash, CoverWash::Running { .. })
            && self.wash_outgoing.is_none()
            && let Some(pixmap) = self.pixmap.as_ref()
            && self.source.keeps_outgoing_during_wash(pixmap)
        {
            self.wash_outgoing = Some(Arc::clone(pixmap));
        }
    }

    fn advance(&mut self, tick: Tick) -> CoverFrame {
        self.hold_wash_outgoing(tick.wash);
        let released = self.wash_outgoing.is_some() && tick.wash == CoverWash::Idle;
        if released {
            self.wash_outgoing = None;
        }
        let stage = self.crossfade.stage(tick.now);
        if stage == CrossfadeStage::Over {
            self.crossfade.finish_if_done(tick.now);
        }
        if released || stage != CrossfadeStage::Idle || self.wash_outgoing.is_some() {
            self.paint_current(tick)
        } else {
            CoverFrame::Keep
        }
    }

    fn paint_current(&mut self, tick: Tick) -> CoverFrame {
        let Some(painted) = self.painted.as_ref() else {
            return CoverFrame::Keep;
        };
        let target = PaintTarget {
            identity: painted.identity.clone(),
            rect: painted.rect,
            tick,
        };
        self.paint(target)
    }

    fn paint(&mut self, target: PaintTarget) -> CoverFrame {
        let Some(pixmap) = self.pixmap.as_ref() else {
            return CoverFrame::Keep;
        };
        let image = self
            .crossfade
            .crossfade_at(pixmap, target.tick.now)
            .or_else(|| self.wash_frame(pixmap, &target))
            .unwrap_or_else(|| RgbaImage::clone(pixmap));
        let image = match self.source {
            PixmapSource::Plain => fit_to_rect(image, target.rect, self.cell),
            PixmapSource::Vinyl(_) => image,
        };
        self.painted = Some(Painted {
            identity: target.identity,
            rect: target.rect,
        });
        CoverFrame::Repaint(image)
    }

    fn wash_frame(
        &self,
        pixmap: &RgbaImage,
        target: &PaintTarget,
    ) -> Option<RgbaImage> {
        let reveal = column_reveal(target.rect, self.cell.width, target.tick.wash)?;
        Some(blend_by_column(
            self.wash_outgoing.as_deref()?,
            pixmap,
            reveal,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use image::{Rgba, RgbaImage};
    use kernel::domain::{appearance::Animations, model::Model, revision::Revision};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        pixels::{
            cover::{
                CoverImage,
                CoverRefresh,
                CoverWash,
                CrossfadePermit,
                lifecycle::{
                    CoverLifecycle,
                    Identity,
                    PaintPlan,
                    PixmapSource,
                    Placed,
                    plan_paint,
                },
                pixmap::CellPixels,
            },
            vinyl::{VinylCacheKey, VinylStyle},
        },
        test_support::SceneSources,
    };

    struct PlanCase {
        painted: Option<(Identity, Rect)>,
        desired: (Identity, Rect),
    }

    fn rect() -> Rect {
        Rect::new(0, 0, 10, 10)
    }

    fn other_rect() -> Rect {
        Rect::new(0, 0, 12, 10)
    }

    fn plain(path: &str) -> Identity {
        Identity::Plain(PathBuf::from(path))
    }

    fn vinyl_key(path: &str, theme_revision: Revision) -> VinylCacheKey {
        VinylCacheKey {
            config_revision: Revision::default(),
            theme_revision,
            path: Some(PathBuf::from(path)),
            size_px: 128,
            colors: VinylStyle::fixture(),
        }
    }

    fn vinyl(path: &str) -> Identity {
        Identity::Vinyl(vinyl_key(path, Revision::default()))
    }

    fn vinyl_with_theme(path: &str, theme_revision: Revision) -> Identity {
        Identity::Vinyl(vinyl_key(path, theme_revision))
    }

    #[rstest]
    #[case::plain_nothing_installed(
        PlanCase { painted: None, desired: (plain("a.jpg"), rect()) },
        PaintPlan::NewContent
    )]
    #[case::plain_same_path_and_rect(
        PlanCase {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("a.jpg"), rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::plain_a_different_path(
        PlanCase {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("b.jpg"), rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::plain_a_different_rect(
        PlanCase {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("a.jpg"), other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_same_key_and_rect(
        PlanCase {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("a.flac"), rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::vinyl_a_different_key(
        PlanCase {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("b.flac"), rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::vinyl_a_different_rect(
        PlanCase {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("a.flac"), other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_only_the_theme_revision_moved(
        PlanCase {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (
                vinyl_with_theme("a.flac", Revision::default().next()),
                rect(),
            ),
        },
        PaintPlan::ThemeWash
    )]
    #[case::vinyl_the_theme_moved_and_the_rect_changed(
        PlanCase {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (
                vinyl_with_theme("a.flac", Revision::default().next()),
                other_rect(),
            ),
        },
        PaintPlan::NewContent
    )]
    fn plan_paint_decides_reuse_or_rebuild(
        #[case] case: PlanCase,
        #[case] expected: PaintPlan,
    ) {
        let painted = case.painted.as_ref().map(|(identity, rect)| Placed {
            identity,
            rect: *rect,
        });
        let desired = Placed {
            identity: &case.desired.0,
            rect: case.desired.1,
        };
        assert_eq!(plan_paint(painted, desired), expected);
    }

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    fn animated_sources() -> SceneSources {
        let mut sources = SceneSources::new(Model::default());
        sources.model.settings.appearance.animations = Animations::On;
        sources
    }

    fn cell() -> CellPixels {
        CellPixels {
            width: 8,
            height: 16,
        }
    }

    fn parts(crossfade: CrossfadePermit) -> CoverRefresh {
        CoverRefresh {
            cover: Some(rect()),
            crossfade,
            wash: CoverWash::Idle,
        }
    }

    #[test]
    fn a_crossfade_keeps_the_incoming_image_shared() {
        let sources = animated_sources();
        let mut cover = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover.set_cover(CoverImage {
            path: PathBuf::from("a.jpg"),
            image: Arc::new(source_pixmap()),
        });
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let second = CoverImage {
            path: PathBuf::from("b.jpg"),
            image: Arc::new(source_pixmap()),
        };
        cover.set_cover(second.clone());
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let incoming = cover.pixmap.as_ref().expect("a pixmap after install");
        assert!(Arc::ptr_eq(incoming, &second.image));
    }

    #[test]
    fn a_settled_vinyl_refresh_shares_the_cached_pixmap() {
        let sources = animated_sources();
        let mut cover =
            CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell());

        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let first = cover
            .pixmap
            .clone()
            .expect("a refresh with a cover rect paints a pixmap");

        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let second = cover
            .pixmap
            .clone()
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
