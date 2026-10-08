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
            crossfade::CoverCrossfade,
            pixmap::{
                BuiltPixmap,
                CellPixels,
                Identity,
                cover_side,
                fit_to_rect,
                plain_pixmap,
            },
            plan::{PaintPlan, PlacedCover, plan_paint},
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
    fn build(
        &mut self,
        wanted: Wanted<'_>,
        painted_cover: Option<&PaintedCover>,
    ) -> Option<BuiltPixmap> {
        match self {
            Self::Plain => plain_pixmap(wanted.cover_image),
            Self::Vinyl(cache) => {
                let key = vinyl_key(&wanted);
                let pixmap = if let Some(PaintedCover {
                    identity: Identity::Vinyl(painted),
                    pixmap,
                    rect: _rect,
                }) = painted_cover
                    && *painted == key
                {
                    Arc::clone(pixmap)
                } else {
                    Arc::new(cache.compose(&wanted))
                };
                Some(BuiltPixmap {
                    pixmap,
                    identity: Identity::Vinyl(key),
                })
            }
        }
    }
}

struct PaintedCover {
    identity: Identity,
    rect: Rect,
    pixmap: Arc<RgbaImage>,
}

impl PaintedCover {
    fn placed(&self) -> PlacedCover<'_> {
        PlacedCover {
            identity: &self.identity,
            rect: self.rect,
        }
    }
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

pub struct CoverLifecycle {
    source: PixmapSource,
    cell_pixels: CellPixels,
    cover_image: Option<CoverImage>,
    painted_cover: Option<PaintedCover>,
    crossfade: Option<CoverCrossfade>,
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
            crossfade: None,
        }
    }

    pub fn set_cover(&mut self, cover_image: CoverImage) {
        self.cover_image = Some(cover_image);
    }

    pub fn refresh(
        &mut self,
        scene: &Scene<'_>,
        cover_area: Option<Rect>,
    ) -> CoverUpdate {
        let Some(rect) = cover_area else {
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
        let since_first_paint = scene.presentation.since_first_paint;
        let placed = self.painted_cover.as_ref().map(PaintedCover::placed);
        if plan_paint(placed, &wanted, rect) == PaintPlan::Reuse {
            return CoverUpdate {
                card_cover: CardCover::Image,
                frame: self.advance(since_first_paint),
            };
        }
        let Some(built_pixmap) = self.source.build(wanted, self.painted_cover.as_ref())
        else {
            return self.forget();
        };
        let pixmap = match self.source {
            PixmapSource::Plain => {
                fit_to_rect(built_pixmap.pixmap, rect, self.cell_pixels)
            }
            PixmapSource::Vinyl(_) => built_pixmap.pixmap,
        };
        let outgoing = if let Some(painted) = self.painted_cover.take()
            && painted.rect == rect
            && painted.identity.path() != wanted.path()
            && scene.settings.appearance_settings.animations == Animations::On
        {
            Some(match self.crossfade.take() {
                Some(crossfade) => crossfade.on_screen(&painted.pixmap),
                None => painted.pixmap,
            })
        } else {
            None
        };
        self.crossfade =
            outgoing.map(|outgoing| CoverCrossfade::begin(outgoing, since_first_paint));
        let frame = match self.crossfade {
            Some(_) => CoverFrame::Keep,
            None => CoverFrame::Repaint(RgbaImage::clone(&pixmap)),
        };
        self.painted_cover = Some(PaintedCover {
            identity: built_pixmap.identity,
            rect,
            pixmap,
        });
        CoverUpdate {
            card_cover: CardCover::Image,
            frame,
        }
    }

    #[must_use]
    pub fn motion(&self) -> CoverMotion {
        self.crossfade
            .as_ref()
            .map_or(CoverMotion::Still, CoverCrossfade::motion)
    }

    fn advance(&mut self, since_first_paint: Duration) -> CoverFrame {
        let (Some(crossfade), Some(painted)) =
            (self.crossfade.as_mut(), self.painted_cover.as_ref())
        else {
            return CoverFrame::Keep;
        };
        let frame = crossfade
            .advance(&painted.pixmap, since_first_paint)
            .map_or(CoverFrame::Keep, CoverFrame::Repaint);
        if crossfade.motion() == CoverMotion::Still {
            self.crossfade = None;
        }
        frame
    }

    fn forget(&mut self) -> CoverUpdate {
        self.painted_cover = None;
        self.crossfade = None;
        CoverUpdate {
            card_cover: CardCover::Missing,
            frame: CoverFrame::Forget,
        }
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
        pixels::cover::{
            CoverImage,
            lifecycle::{CoverFrame, CoverLifecycle, PixmapSource},
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

    fn sources() -> SceneSources {
        SceneSources::new(Model::default())
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
        let mut sources = sources();
        sources.model.player = current.map_or(Player::Stopped, playing);
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image(decoded));

        let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        assert_eq!(
            mem::discriminant(&update.card_cover),
            mem::discriminant(&expected)
        );
    }

    #[test]
    fn a_track_change_without_a_new_cover_clears_the_previous_art() {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");

        let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
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
    fn a_new_cover_is_painted_as_it_is() {
        let mut sources = sources();
        sources.model.settings.appearance_settings.animations = Animations::Off;
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));

        let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        assert_eq!(painted_pixel(&update.frame), Some(Rgba([20, 100, 50, 128])));
    }

    fn painted_reds_over_a_cover_change(sources: &mut SceneSources) -> Vec<Option<u8>> {
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(CoverImage {
            path: PathBuf::from("/music/b.flac"),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([20, 100, 50, 255]))),
        });
        (0..=28)
            .map(|tick| {
                sources.since_first_paint = Duration::from_millis(25 * tick);
                let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
                assert!(matches!(update.card_cover, CardCover::Image));
                assert!(!matches!(update.frame, CoverFrame::Forget));
                painted_pixel(&update.frame).map(|Rgba([red, ..])| red)
            })
            .collect()
    }

    #[test]
    fn a_cover_change_with_animations_on_repaints_in_eight_steps() {
        let mut sources = sources();

        let reds = painted_reds_over_a_cover_change(&mut sources);
        let repaints: Vec<u8> = reds.iter().flatten().copied().collect();
        assert_eq!(repaints.len(), 8);
        assert!(repaints.is_sorted_by(|a, b| a > b));
        assert!(repaints[..7].iter().all(|red| (21..200).contains(red)));
        assert_eq!(repaints.last(), Some(&20));
        assert_eq!(reds.first(), Some(&None));
        assert_eq!(reds.iter().rposition(Option::is_some), Some(24));
    }

    #[test]
    fn a_cover_change_with_animations_off_repaints_once() {
        let mut sources = sources();
        sources.model.settings.appearance_settings.animations = Animations::Off;

        let reds = painted_reds_over_a_cover_change(&mut sources);
        let repaints: Vec<u8> = reds.iter().flatten().copied().collect();
        assert_eq!(repaints, vec![20]);
        assert_eq!(reds.first(), Some(&Some(20)));
    }

    #[test]
    fn a_cover_change_mid_fade_starts_the_next_fade_from_the_blended_cover() {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(CoverImage {
            path: PathBuf::from("/music/b.flac"),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([20, 100, 50, 255]))),
        });
        let first_fade_reds: Vec<u8> = (0..=12)
            .filter_map(|tick| {
                sources.since_first_paint = Duration::from_millis(25 * tick);
                let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
                painted_pixel(&update.frame).map(|Rgba([red, ..])| red)
            })
            .collect();
        let blended = first_fade_reds.last().copied();
        assert!(blended.is_some_and(|red| (21..200).contains(&red)));
        sources.model.player = playing("/music/c.flac");
        cover_lifecycle.set_cover(CoverImage {
            path: PathBuf::from("/music/c.flac"),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([250, 100, 50, 255]))),
        });
        let next_fade_red = (12..=36).find_map(|tick| {
            sources.since_first_paint = Duration::from_millis(25 * tick);
            let update = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
            painted_pixel(&update.frame).map(|Rgba([red, ..])| red)
        });
        assert!(
            blended
                .zip(next_fade_red)
                .is_some_and(|(blended, red)| blended < red && red < 250),
            "blended {blended:?}, first repaint of the next fade {next_fade_red:?}"
        );
    }

    fn vinyl_repaints_over(
        change: fn(&mut SceneSources, &mut CoverLifecycle),
    ) -> usize {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle =
            CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        change(&mut sources, &mut cover_lifecycle);
        (0..=28)
            .filter(|tick| {
                sources.since_first_paint = Duration::from_millis(25 * tick);
                matches!(
                    cover_lifecycle
                        .refresh(&sources.scene(), Some(rect()))
                        .frame,
                    CoverFrame::Repaint(_)
                )
            })
            .count()
    }

    fn restyle(sources: &mut SceneSources, _: &mut CoverLifecycle) {
        sources.theme.colors.accent = Rgb([0x20, 0x90, 0xd0]);
    }

    fn change_track(sources: &mut SceneSources, cover_lifecycle: &mut CoverLifecycle) {
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));
    }

    #[rstest]
    #[case::a_theme_change_on_a_painted_vinyl_repaints_once(restyle, 1)]
    #[case::a_track_change_on_a_vinyl_repaints_in_eight_steps(change_track, 8)]
    fn a_vinyl_with_animations_on_fades_only_a_cover_change(
        #[case] change: fn(&mut SceneSources, &mut CoverLifecycle),
        #[case] repaints: usize,
    ) {
        assert_eq!(vinyl_repaints_over(change), repaints);
    }

    #[test]
    fn a_track_change_fits_the_new_pixmap_to_the_cover_rect() {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(cover_image("/music/b.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        let incoming = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| &painted.pixmap)
            .expect("a pixmap after install");
        assert_eq!(incoming.dimensions(), fitted_size());
    }

    #[test]
    fn a_new_cover_in_a_resized_rect_is_painted_as_it_is() {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));
        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        sources.model.player = playing("/music/b.flac");
        cover_lifecycle.set_cover(translucent_cover("/music/b.flac", 20));
        let update =
            cover_lifecycle.refresh(&sources.scene(), Some(Rect::new(0, 0, 6, 6)));
        assert_eq!(painted_pixel(&update.frame), Some(Rgba([20, 100, 50, 128])));
    }

    #[test]
    fn a_settled_plain_refresh_keeps_the_fitted_pixmap_and_repaints_nothing() {
        let mut sources = sources();
        sources.model.player = playing("/music/a.flac");
        let mut cover_lifecycle = CoverLifecycle::new(PixmapSource::Plain, cell());
        cover_lifecycle.set_cover(cover_image("/music/a.flac"));

        let first = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        let fitted = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");
        let second = cover_lifecycle.refresh(&sources.scene(), Some(rect()));
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
        let sources = sources();
        let mut cover_lifecycle =
            CoverLifecycle::new(PixmapSource::Vinyl(Box::default()), cell());

        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        let first = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a refresh with a cover rect paints a pixmap");

        cover_lifecycle.refresh(&sources.scene(), Some(rect()));
        let second = cover_lifecycle
            .painted_cover
            .as_ref()
            .map(|painted| Arc::clone(&painted.pixmap))
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
