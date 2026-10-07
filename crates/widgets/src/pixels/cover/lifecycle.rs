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
                cover_side,
                fit_to_rect,
                is_translucent,
                plain_pixmap,
            },
            plan::{PaintPlan, PlacedCover, plan_paint},
            wash::column_reveal,
        },
        vinyl::{VinylCache, VinylStyle, Wanted, vinyl_key},
    },
    scene::Scene,
};

#[derive(Debug)]
pub enum PixmapSource {
    Plain,
    Vinyl(Box<VinylCache>),
}

impl PixmapSource {
    fn keeps_outgoing_during_wash(&self, translucency: Translucency) -> bool {
        match self {
            Self::Plain => translucency == Translucency::Translucent,
            Self::Vinyl(_) => false,
        }
    }

    fn build(
        &mut self,
        wanted: Wanted<'_>,
        painted_cover: Option<&PaintedCover>,
    ) -> Option<BuiltPixmap> {
        match self {
            Self::Plain => plain_pixmap(wanted.cover_image),
            Self::Vinyl(cache) => {
                let key = vinyl_key(&wanted);
                let pixmap = match painted_cover {
                    Some(PaintedCover {
                        identity: Identity::Vinyl(painted),
                        pixmap,
                        rect: _rect,
                        translucency: _translucency,
                    }) if *painted == key => Arc::clone(pixmap),
                    Some(_) | None => Arc::new(cache.compose(&wanted)),
                };
                Some(BuiltPixmap {
                    pixmap,
                    identity: Identity::Vinyl(key),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Tick {
    since_first_paint: Duration,
    wash: CoverWash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Translucency {
    Opaque,
    Translucent,
}

struct PaintedCover {
    identity: Identity,
    rect: Rect,
    pixmap: Arc<RgbaImage>,
    translucency: Translucency,
}

impl PaintedCover {
    fn placed(&self) -> PlacedCover<'_> {
        PlacedCover {
            identity: &self.identity,
            rect: self.rect,
        }
    }
}

struct RebuildParts {
    plan: PaintPlan,
    built_pixmap: BuiltPixmap,
    rect: Rect,
    crossfade_permit: CrossfadePermit,
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
    pub card_cover: CardCover,
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
    cell_pixels: CellPixels,
    cover_image: Option<CoverImage>,
    painted_cover: Option<PaintedCover>,
    fade: CoverFade,
}

impl fmt::Debug for CoverLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoverLifecycle")
            .field(
                "painted",
                &self
                    .painted_cover
                    .as_ref()
                    .map(|painted| (&painted.identity, painted.rect)),
            )
            .finish()
    }
}

impl CoverLifecycle {
    #[must_use]
    pub fn new(source: PixmapSource, cell_pixels: CellPixels) -> Self {
        Self {
            source,
            cell_pixels,
            cover_image: None,
            painted_cover: None,
            fade: CoverFade::Still,
        }
    }

    pub fn set_cover(&mut self, cover_image: CoverImage) {
        self.cover_image = Some(cover_image);
    }

    pub fn refresh(
        &mut self,
        scene: &Scene<'_>,
        cover_refresh: CoverRefresh,
    ) -> CoverUpdate {
        let CoverRefresh {
            cover_area: cover,
            crossfade_permit,
            wash,
        } = cover_refresh;
        let Some(rect) = cover else {
            return self.forget();
        };
        let current = scene.current_track_path();
        let wanted = Wanted {
            cover_image: self
                .cover_image
                .as_ref()
                .filter(|cover_image| Some(cover_image.path.as_path()) == current),
            side: cover_side(rect, self.cell_pixels),
            vinyl_style: VinylStyle::from_theme(&scene.active_theme()),
        };
        let tick = Tick {
            since_first_paint: scene.presentation.since_first_paint,
            wash,
        };
        let placed = self.painted_cover.as_ref().map(PaintedCover::placed);
        let plan = plan_paint(placed, &wanted, rect);
        if plan == PaintPlan::Reuse {
            return CoverUpdate {
                card_cover: CardCover::Image,
                frame: self.advance(tick),
            };
        }
        let Some(built_pixmap) = self.source.build(wanted, self.painted_cover.as_ref())
        else {
            return self.forget();
        };
        let crossfade_permit = if scene.settings.appearance_settings.animations
            == Animations::On
            && plan == PaintPlan::Rebuild
        {
            crossfade_permit
        } else {
            CrossfadePermit::Withheld
        };
        let frame = self.rebuild(RebuildParts {
            plan,
            built_pixmap,
            rect,
            crossfade_permit,
            tick,
        });
        CoverUpdate {
            card_cover: CardCover::Image,
            frame,
        }
    }

    #[must_use]
    pub fn motion(&self) -> CoverMotion {
        match &self.fade {
            CoverFade::Still => CoverMotion::Still,
            CoverFade::Crossfade(_) | CoverFade::Wash(_) => CoverMotion::Moving,
        }
    }

    fn forget(&mut self) -> CoverUpdate {
        self.painted_cover = None;
        self.fade = CoverFade::Still;
        CoverUpdate {
            card_cover: CardCover::Missing,
            frame: CoverFrame::Forget,
        }
    }

    fn rebuild(&mut self, parts: RebuildParts) -> CoverFrame {
        let RebuildParts {
            plan,
            built_pixmap,
            rect,
            crossfade_permit,
            tick,
        } = parts;
        let outgoing = self.painted_cover.take().map(|painted| painted.pixmap);
        let pixmap = match self.source {
            PixmapSource::Plain => {
                fit_to_rect(built_pixmap.pixmap, rect, self.cell_pixels)
            }
            PixmapSource::Vinyl(_) => built_pixmap.pixmap,
        };
        let shown = if plan == PaintPlan::Wash {
            self.fade = match outgoing {
                Some(outgoing) if matches!(tick.wash, CoverWash::Running { .. }) => {
                    CoverFade::Wash(outgoing)
                }
                Some(_) | None => CoverFade::Still,
            };
            tick
        } else {
            self.fade = match outgoing {
                Some(outgoing)
                    if crossfade_permit == CrossfadePermit::Allowed
                        && outgoing.dimensions() == pixmap.dimensions() =>
                {
                    CoverFade::Crossfade(CoverCrossfade::begin(
                        outgoing,
                        tick.since_first_paint,
                    ))
                }
                Some(_) | None => CoverFade::Still,
            };
            Tick {
                wash: CoverWash::Idle,
                ..tick
            }
        };
        let translucency = if is_translucent(&pixmap) {
            Translucency::Translucent
        } else {
            Translucency::Opaque
        };
        self.painted_cover = Some(PaintedCover {
            identity: built_pixmap.identity,
            rect,
            pixmap,
            translucency,
        });
        let frame = self.cover_frame(shown);
        self.hold_wash_outgoing(tick);
        frame
    }

    fn hold_wash_outgoing(&mut self, tick: Tick) {
        if matches!(tick.wash, CoverWash::Running { .. })
            && match &self.fade {
                CoverFade::Still => true,
                CoverFade::Crossfade(fade) => {
                    fade.stage(tick.since_first_paint) == CrossfadeStage::Ended
                }
                CoverFade::Wash(_) => false,
            }
            && let Some(PaintedCover {
                pixmap,
                translucency,
                identity: _identity,
                rect: _rect,
            }) = &self.painted_cover
            && self.source.keeps_outgoing_during_wash(*translucency)
        {
            self.fade = CoverFade::Wash(Arc::clone(pixmap));
        }
    }

    fn advance(&mut self, tick: Tick) -> CoverFrame {
        self.hold_wash_outgoing(tick);
        match &self.fade {
            CoverFade::Still => CoverFrame::Keep,
            CoverFade::Crossfade(crossfade) => {
                if crossfade.stage(tick.since_first_paint) == CrossfadeStage::Ended {
                    self.fade = CoverFade::Still;
                }
                self.cover_frame(tick)
            }
            CoverFade::Wash(_) => {
                if tick.wash == CoverWash::Idle {
                    self.fade = CoverFade::Still;
                }
                self.cover_frame(tick)
            }
        }
    }

    fn cover_frame(&self, tick: Tick) -> CoverFrame {
        let Some(painted) = self.painted_cover.as_ref() else {
            return CoverFrame::Keep;
        };
        let pixmap = &painted.pixmap;
        let image = match &self.fade {
            CoverFade::Still => RgbaImage::clone(pixmap),
            CoverFade::Crossfade(crossfade) => {
                crossfade.crossfade_at(pixmap, tick.since_first_paint)
            }
            CoverFade::Wash(outgoing) => {
                column_reveal(painted.rect, self.cell_pixels.width, tick.wash)
                    .map_or_else(
                        || RgbaImage::clone(pixmap),
                        |reveal| blend_by_column(outgoing, pixmap, reveal),
                    )
            }
        };
        CoverFrame::Repaint(image)
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
        appearance::Animations,
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
        pixels::cover::{
            CoverImage,
            CoverRefresh,
            CoverWash,
            CrossfadePermit,
            lifecycle::{CoverFade, CoverFrame, CoverLifecycle, PixmapSource},
            pixmap::CellPixels,
        },
        test_support::SceneSources,
    };

    fn rect() -> Rect {
        Rect::new(0, 0, 10, 10)
    }

    fn source_pixmap() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))
    }

    fn animated_sources() -> SceneSources {
        let mut sources = SceneSources::new(Model::default());
        sources.model.settings.appearance_settings.animations = Animations::On;
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

    fn parts(crossfade_permit: CrossfadePermit) -> CoverRefresh {
        CoverRefresh {
            cover_area: Some(rect()),
            crossfade_permit,
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
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image(decoded));

        let update =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        assert_eq!(
            mem::discriminant(&update.card_cover),
            mem::discriminant(&expected)
        );
    }

    #[test]
    fn a_track_change_without_a_new_cover_clears_the_previous_art() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        sources.model.player = playing("/music/b.flac");

        let update =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        assert!(matches!(update.card_cover, CardCover::Missing));
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
        over.presentation.since_first_paint = Duration::from_secs(5);
        let washed = cover_lifecycle.refresh(&over, cover_refresh);
        assert!(matches!(cover_lifecycle.fade, CoverFade::Wash(_)));
        assert_eq!(painted_pixel(&washed.frame), Some(Rgba([20, 100, 50, 128])));
    }

    #[test]
    fn a_crossfade_blends_a_pixmap_already_fitted_to_the_cover_rect() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(cover_image("/music/b.flac"));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let incoming = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| &painted.pixmap)
            .expect("a pixmap after install");
        assert_eq!(incoming.dimensions(), fitted_size());
    }

    #[test]
    fn a_new_cover_in_a_resized_rect_shows_still_instead_of_crossfading() {
        let mut sources = animated_sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));
        let resized_cover_refresh = CoverRefresh {
            cover_area: Some(Rect::new(0, 0, 6, 6)),
            ..parts(CrossfadePermit::Allowed)
        };

        let update = cover_lifecycle.refresh(&sources.scene(), resized_cover_refresh);
        assert!(matches!(cover_lifecycle.fade, CoverFade::Still));
        assert_eq!(painted_pixel(&update.frame), Some(Rgba([20, 100, 50, 128])));
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
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");
        let second =
            cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Withheld));
        let kept = cover_lifecycle
            .painted_cover
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
        let mut cover_lifecycle =
            CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell());

        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let first = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");

        cover_lifecycle.refresh(&sources.scene(), parts(CrossfadePermit::Allowed));
        let second = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
