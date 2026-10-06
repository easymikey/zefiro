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
                Wanted,
                compose_vinyl,
                fit_to_rect,
                plain_pixmap,
                translucent,
                vinyl_key,
                vinyl_size,
            },
            wash::column_reveal,
        },
        vinyl::{VinylCache, VinylStyle},
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

    fn build(
        &mut self,
        wanted: Wanted<'_>,
        painted: Option<&Painted>,
    ) -> Option<BuiltPixmap> {
        match self {
            Self::Plain => plain_pixmap(wanted.cover_image),
            Self::Vinyl(cache) => {
                let key = vinyl_key(&wanted);
                let pixmap = match painted {
                    Some(Painted {
                        identity: Identity::Vinyl(painted),
                        pixmap,
                        ..
                    }) if *painted == key => Arc::clone(pixmap),
                    Some(_) | None => compose_vinyl(cache, &key, wanted.cover_image),
                };
                Some(BuiltPixmap {
                    pixmap,
                    identity: Identity::Vinyl(key),
                })
            }
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

#[derive(Debug, Clone, Copy)]
struct Placed<'a> {
    identity: &'a Identity,
    rect: Rect,
}

#[must_use]
fn plan_paint(
    painted: Option<Placed<'_>>,
    wanted: &Wanted<'_>,
    rect: Rect,
) -> PaintPlan {
    match painted {
        Some(painted) if painted.rect == rect && painted.identity.is(wanted) => {
            PaintPlan::Reuse
        }
        Some(painted)
            if painted.rect == rect
                && painted.identity.changed_only_by_theme(wanted) =>
        {
            PaintPlan::ThemeWash
        }
        Some(painted) if painted.identity.is(wanted) => PaintPlan::SameContent,
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

#[derive(Debug)]
enum CoverFade {
    Still,
    Crossfade(CoverCrossfade),
    Wash(Arc<RgbaImage>),
}

pub struct CoverLifecycle {
    source: PixmapSource,
    cell: CellPixels,
    decoded: Option<CoverImage>,
    painted: Option<Painted>,
    fade: CoverFade,
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
            fade: CoverFade::Still,
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
        let current = scene.current_track_path();
        let wanted = Wanted {
            cover_image: self
                .decoded
                .as_ref()
                .filter(|decoded| Some(decoded.path.as_path()) == current),
            pixels: vinyl_size(rect, self.cell),
            vinyl_style: VinylStyle::from_theme(&scene.active_theme()),
        };
        let tick = Tick {
            now: scene.presentation.clock,
            wash,
        };
        let plan =
            plan_paint(self.painted.as_ref().map(Painted::placed), &wanted, rect);
        if plan == PaintPlan::Reuse {
            return CoverUpdate {
                art: CardCover::Image,
                frame: self.advance(tick),
            };
        }
        let Some(built) = self.source.build(wanted, self.painted.as_ref()) else {
            return self.forget();
        };
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
        match &self.fade {
            CoverFade::Still => CoverMotion::Still,
            CoverFade::Wash(_) => CoverMotion::Animating,
            CoverFade::Crossfade(crossfade) => match crossfade.stage(now) {
                CrossfadeStage::Running | CrossfadeStage::Over => {
                    CoverMotion::Animating
                }
            },
        }
    }

    fn forget(&mut self) -> CoverUpdate {
        self.painted = None;
        self.fade = CoverFade::Still;
        CoverUpdate {
            art: CardCover::Missing,
            frame: CoverFrame::Forget,
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
        let shown = if plan == PaintPlan::ThemeWash {
            self.fade = match outgoing {
                Some(outgoing) if matches!(tick.wash, CoverWash::Running { .. }) => {
                    CoverFade::Wash(outgoing)
                }
                Some(_) | None => CoverFade::Still,
            };
            tick
        } else {
            self.fade = match outgoing {
                Some(outgoing) if crossfade == CrossfadePermit::Allowed => {
                    CoverFade::Crossfade(CoverCrossfade::begin(outgoing, tick.now))
                }
                Some(_) | None => CoverFade::Still,
            };
            Tick {
                wash: CoverWash::Idle,
                ..tick
            }
        };
        let pixmap = match self.source {
            PixmapSource::Plain => fit_to_rect(built.pixmap, rect, self.cell),
            PixmapSource::Vinyl(_) => built.pixmap,
        };
        let frame = self.paint(PaintTarget {
            painted: Painted {
                identity: built.identity,
                rect,
                pixmap,
            },
            tick: shown,
        });
        self.hold_wash_outgoing(tick);
        frame
    }

    fn hold_wash_outgoing(&mut self, tick: Tick) {
        if matches!(tick.wash, CoverWash::Running { .. })
            && match &self.fade {
                CoverFade::Still => true,
                CoverFade::Crossfade(fade) => {
                    fade.stage(tick.now) == CrossfadeStage::Over
                }
                CoverFade::Wash(_) => false,
            }
            && let Some(pixmap) = self.painted.as_ref().map(|painted| &painted.pixmap)
            && self.source.keeps_outgoing_during_wash(pixmap)
        {
            self.fade = CoverFade::Wash(Arc::clone(pixmap));
        }
    }

    fn advance(&mut self, tick: Tick) -> CoverFrame {
        self.hold_wash_outgoing(tick);
        match &self.fade {
            CoverFade::Still => CoverFrame::Keep,
            CoverFade::Crossfade(crossfade) => {
                if crossfade.stage(tick.now) == CrossfadeStage::Over {
                    self.fade = CoverFade::Still;
                }
                self.paint_current(tick)
            }
            CoverFade::Wash(_) => {
                if tick.wash == CoverWash::Idle {
                    self.fade = CoverFade::Still;
                }
                self.paint_current(tick)
            }
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
        let image = match &self.fade {
            CoverFade::Still => RgbaImage::clone(pixmap),
            CoverFade::Crossfade(crossfade) => {
                crossfade.crossfade_at(pixmap, target.tick.now)
            }
            CoverFade::Wash(outgoing) => self
                .wash_frame(outgoing, &target)
                .unwrap_or_else(|| RgbaImage::clone(pixmap)),
        };
        self.painted = Some(target.painted);
        CoverFrame::Repaint(image)
    }

    fn wash_frame(
        &self,
        outgoing: &RgbaImage,
        target: &PaintTarget,
    ) -> Option<RgbaImage> {
        let reveal =
            column_reveal(target.painted.rect, self.cell.width, target.tick.wash)?;
        Some(blend_by_column(outgoing, &target.painted.pixmap, reveal))
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
        geometry::{Cells, Pixels},
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
                    CoverFade,
                    CoverFrame,
                    CoverLifecycle,
                    PaintPlan,
                    PixmapSource,
                    Placed,
                    plan_paint,
                },
                pixmap::{CellPixels, Identity, Wanted},
            },
            vinyl::{VinylCacheKey, VinylStyle},
        },
        test_support::SceneSources,
    };

    struct Want {
        path: &'static str,
        vinyl_style: VinylStyle,
        rect: Rect,
    }

    struct PlanRow {
        painted: Option<(Identity, Rect)>,
        want: Want,
    }

    fn want(path: &'static str, rect: Rect) -> Want {
        Want {
            path,
            vinyl_style: VinylStyle::fixture(),
            rect,
        }
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
        PlanRow { painted: None, want: want("a.jpg", rect()) },
        PaintPlan::NewContent
    )]
    #[case::plain_same_path_and_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("a.jpg", rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::plain_a_different_path(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("b.jpg", rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::plain_a_different_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("a.jpg", other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_same_key_and_rect(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: want("a.flac", rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::vinyl_a_different_key(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: want("b.flac", rect()),
        },
        PaintPlan::NewContent
    )]
    #[case::vinyl_a_different_rect(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: want("a.flac", other_rect()),
        },
        PaintPlan::SameContent
    )]
    #[case::vinyl_only_the_colors_moved(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: Want { vinyl_style: recolored(), ..want("a.flac", rect()) },
        },
        PaintPlan::ThemeWash
    )]
    #[case::vinyl_the_theme_moved_and_the_rect_changed(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: Want { vinyl_style: recolored(), ..want("a.flac", other_rect()) },
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
        let cover = cover_image(case.want.path);
        let wanted = Wanted {
            cover_image: Some(&cover),
            pixels: Pixels(128),
            vinyl_style: case.want.vinyl_style,
        };
        assert_eq!(plan_paint(painted, &wanted, case.want.rect), expected);
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

    fn fitted_size() -> (u32, u32) {
        (
            u32::from(rect().width) * cell().width.0,
            u32::from(rect().height) * cell().height.0,
        )
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

    fn translucent_cover(path: &str, red: u8) -> CoverImage {
        CoverImage {
            path: PathBuf::from(path),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([red, 100, 50, 128]))),
        }
    }

    fn painted_pixel(frame: &CoverFrame) -> Option<Rgba<u8>> {
        match frame {
            CoverFrame::Repaint(image) => image.get_pixel_checked(1, 1).copied(),
            CoverFrame::Keep | CoverFrame::Forget => None,
        }
    }

    #[test]
    fn a_cover_with_no_crossfade_behind_it_is_painted_as_it_is() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));

        let update =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));

        assert!(matches!(cover_lifecycle.fade, CoverFade::Still));
        assert_eq!(painted_pixel(&update.frame), Some(Rgba([20, 100, 50, 128])));
    }

    #[test]
    fn a_wash_arriving_during_a_crossfade_waits_for_the_crossfade() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(translucent_cover("/music/a.flac", 200));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let cover_refresh = CoverRefresh {
            wash: CoverWash::Running {
                progress: 0.0,
                screen_width: Cells(80),
            },
            ..parts(CrossfadePermit::Allowed)
        };

        let outgoing = Rgba([200, 100, 50, 128]);

        let waiting = cover_lifecycle.refresh(&sources.scene(), cover_refresh);

        assert_eq!(painted_pixel(&waiting.frame), Some(outgoing));
        let mut over = sources.scene();
        over.presentation.clock = Duration::from_secs(5);
        let washed = cover_lifecycle.refresh(&over, cover_refresh);
        assert!(matches!(cover_lifecycle.fade, CoverFade::Wash(_)));
        assert_eq!(painted_pixel(&washed.frame), Some(Rgba([20, 100, 50, 128])));
    }

    #[test]
    fn a_crossfade_blends_a_pixmap_already_fitted_to_the_cover_rect() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover.set_cover(cover_image("/music/a.flac"));
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        sources.model.player = playing("/music/b.flac");
        cover.set_cover(cover_image("/music/b.flac"));
        cover.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let incoming = cover
            .painted
            .as_ref()
            .map(|painted| &painted.pixmap)
            .expect("a pixmap after install");
        assert_eq!(incoming.dimensions(), fitted_size());
    }

    #[test]
    fn a_settled_plain_refresh_keeps_the_fitted_pixmap_and_repaints_nothing() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));

        let first =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        let fitted = cover_lifecycle
            .painted
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");
        let second =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        let kept = cover_lifecycle
            .painted
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(matches!(first.frame, CoverFrame::Repaint(_)));
        assert_eq!(fitted.dimensions(), fitted_size());
        assert!(matches!(second.frame, CoverFrame::Keep));
        assert!(Arc::ptr_eq(&fitted, &kept));
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
