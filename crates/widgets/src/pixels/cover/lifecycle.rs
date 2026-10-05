use std::{fmt, sync::Arc, time::Duration};

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
                BuiltPixmap,
                CellPixels,
                Identity,
                compose_vinyl,
                fit_to_rect,
                plain_pixmap,
                translucent,
                vinyl_key,
                vinyl_size,
            },
            wash::column_reveal,
        },
        vinyl::VinylCache,
    },
    scene::Scene,
};

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
    pixmap: Arc<RgbaImage>,
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
    painted: Painted,
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
            crossfade: CoverCrossfade::default(),
            wash_outgoing: None,
        }
    }

    pub fn set_cover(&mut self, decoded: CoverImage) {
        self.decoded = Some(decoded);
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
            now: scene.presentation.clock,
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
        let crossfade = if scene.settings.appearance.animations == Animations::On
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
        self.painted = None;
        self.wash_outgoing = None;
        self.crossfade = CoverCrossfade::default();
        CoverUpdate {
            art: CardCover::Missing,
            frame: CoverFrame::Forget,
        }
    }

    fn build(&mut self, scene: &Scene<'_>, rect: Rect) -> Option<BuiltPixmap> {
        let current = scene.current_track_path();
        let decoded = self
            .decoded
            .as_ref()
            .filter(|cover| Some(cover.path.as_path()) == current);
        match &mut self.source {
            PixmapSource::Plain => plain_pixmap(decoded),
            PixmapSource::Vinyl(cache) => {
                let size = vinyl_size(rect, self.cell);
                let key = vinyl_key(scene, decoded, size);
                let pixmap = match &self.painted {
                    Some(Painted {
                        identity: Identity::Vinyl(painted),
                        pixmap,
                        ..
                    }) if *painted == key => Arc::clone(pixmap),
                    Some(_) | None => compose_vinyl(cache, &key, decoded),
                };
                Some(BuiltPixmap {
                    pixmap,
                    identity: Identity::Vinyl(key),
                })
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
        let outgoing = self.painted.take().map(|painted| painted.pixmap);
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
            painted: Painted {
                identity: built.identity,
                rect,
                pixmap: built.pixmap,
            },
            tick: shown,
        });
        self.hold_wash_outgoing(tick.wash);
        frame
    }

    fn hold_wash_outgoing(&mut self, wash: CoverWash) {
        if matches!(wash, CoverWash::Running { .. })
            && self.wash_outgoing.is_none()
            && let Some(pixmap) = self.painted.as_ref().map(|painted| &painted.pixmap)
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
            painted: Painted {
                identity: painted.identity.clone(),
                rect: painted.rect,
                pixmap: Arc::clone(&painted.pixmap),
            },
            tick,
        };
        self.paint(target)
    }

    fn paint(&mut self, target: PaintTarget) -> CoverFrame {
        let pixmap = &target.painted.pixmap;
        let image = self
            .crossfade
            .crossfade_at(pixmap, target.tick.now)
            .or_else(|| self.wash_frame(pixmap, &target))
            .unwrap_or_else(|| RgbaImage::clone(pixmap));
        let image = match self.source {
            PixmapSource::Plain => fit_to_rect(image, target.painted.rect, self.cell),
            PixmapSource::Vinyl(_) => image,
        };
        self.painted = Some(target.painted);
        CoverFrame::Repaint(image)
    }

    fn wash_frame(
        &self,
        pixmap: &RgbaImage,
        target: &PaintTarget,
    ) -> Option<RgbaImage> {
        let reveal =
            column_reveal(target.painted.rect, self.cell.width, target.tick.wash)?;
        Some(blend_by_column(
            self.wash_outgoing.as_deref()?,
            pixmap,
            reveal,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        mem,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use image::{Rgba, RgbaImage};
    use kernel::domain::{
        appearance::{Animations, Rgb},
        geometry::Pixels,
        model::Model,
        player::Player,
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        track::Track,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        card::CardCover,
        pixels::{
            cover::{
                CoverImage,
                CoverRefresh,
                CoverWash,
                CrossfadePermit,
                lifecycle::{
                    CoverLifecycle,
                    PaintPlan,
                    PixmapSource,
                    Placed,
                    plan_paint,
                },
                pixmap::{CellPixels, Identity},
            },
            vinyl::{VinylCacheKey, VinylStyle},
        },
        test_support::SceneSources,
    };

    struct PlanRow {
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

    fn vinyl_with_colors(path: &str, colors: VinylStyle) -> Identity {
        Identity::Vinyl(VinylCacheKey {
            path: Some(PathBuf::from(path)),
            size: Pixels(128),
            colors,
        })
    }

    fn vinyl(path: &str) -> Identity {
        vinyl_with_colors(path, VinylStyle::fixture())
    }

    fn recolored() -> VinylStyle {
        VinylStyle {
            accent: Rgb([0x3d, 0x9b, 0xff]),
            ..VinylStyle::fixture()
        }
    }

    #[rstest]
    #[case::plain_nothing_installed(
        PlanRow { painted: None, desired: (plain("a.jpg"), rect()) },
        PaintPlan::NewContent
    )]
    #[case::plain_same_path_and_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("a.jpg"), rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::plain_a_different_path(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("b.jpg"), rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::plain_a_different_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            desired: (plain("a.jpg"), other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_same_key_and_rect(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("a.flac"), rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::vinyl_a_different_key(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("b.flac"), rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::vinyl_a_different_rect(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl("a.flac"), other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_only_the_colors_moved(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl_with_colors("a.flac", recolored()), rect()),
        },
        PaintPlan::ThemeWash
    )]
    #[case::vinyl_the_theme_moved_and_the_rect_changed(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            desired: (vinyl_with_colors("a.flac", recolored()), other_rect()),
        },
        PaintPlan::NewContent
    )]
    fn plan_paint_decides_reuse_or_rebuild(
        #[case] case: PlanRow,
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
            width: Pixels(8),
            height: Pixels(16),
        }
    }

    fn parts(crossfade: CrossfadePermit) -> CoverRefresh {
        CoverRefresh {
            cover: Some(rect()),
            crossfade,
            wash: CoverWash::Idle,
        }
    }

    fn playing(path: &str) -> Player {
        Player::Playing {
            track: Arc::new(Track::listed(Path::new(path))),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        }
    }

    fn cover_image(path: &str) -> CoverImage {
        CoverImage {
            path: PathBuf::from(path),
            image: Arc::new(source_pixmap()),
        }
    }

    #[rstest]
    #[case::the_current_tracks_cover_is_shown(
        "/music/a.flac",
        Some("/music/a.flac"),
        CardCover::Image
    )]
    #[case::a_cover_decoded_for_another_track_is_not_shown(
        "/music/b.flac",
        Some("/music/a.flac"),
        CardCover::Missing
    )]
    #[case::a_cover_with_no_current_track_is_not_shown(
        "/music/a.flac",
        None,
        CardCover::Missing
    )]
    fn a_plain_cover_shows_the_decoded_art_only_for_the_current_track(
        #[case] decoded: &str,
        #[case] current: Option<&str>,
        #[case] expected: CardCover,
    ) {
        let mut sources = animated_sources();
        sources.model.player = current.map_or(Player::Stopped, playing);
        let mut cover = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover.set_cover(cover_image(decoded));

        let update = cover.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));

        assert_eq!(mem::discriminant(&update.art), mem::discriminant(&expected));
    }

    #[test]
    fn a_track_change_without_a_new_cover_clears_the_previous_art() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover.set_cover(cover_image("/music/a.flac"));
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        sources.model.player = playing("/music/b.flac");

        let update = cover.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));

        assert!(matches!(update.art, CardCover::Missing));
    }

    #[test]
    fn a_crossfade_keeps_the_incoming_image_shared() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover.set_cover(cover_image("/music/a.flac"));
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        sources.model.player = playing("/music/b.flac");
        let second = cover_image("/music/b.flac");
        cover.set_cover(second.clone());
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let incoming = cover
            .painted
            .as_ref()
            .map(|painted| &painted.pixmap)
            .expect("a pixmap after install");
        assert!(Arc::ptr_eq(incoming, &second.image));
    }

    #[test]
    fn a_settled_vinyl_refresh_shares_the_cached_pixmap() {
        let sources = animated_sources();
        let mut cover =
            CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell());

        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let first = cover
            .painted
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");

        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let second = cover
            .painted
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
