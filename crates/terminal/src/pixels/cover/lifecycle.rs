use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use config::Animations;
use image::{DynamicImage, RgbaImage};
use raster::{VinylCache, VinylCacheKey, VinylColors};
use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use widgets::FrameLayout;

use crate::pixels::cover::{
    CoverFade,
    CoverKey,
    CoverMotion,
    CoverWash,
    DecodedCover,
    OwnedCoverArt,
    crossfade::{CoverCrossfade, CrossfadeStage},
    pixel::{fit_to_rect, plain_pixmap, translucent},
    protocol::cover_protocol,
    vinyl::{VinylComposeParts, compose_vinyl, key_changed_only_by_theme},
    wash::{WashFrame, wash_frame},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CoverRefresh<'a> {
    pub(crate) key: CoverKey,
    pub(crate) colors: VinylColors,
    pub(crate) clock: Duration,
    pub(crate) animations: Animations,
    pub(crate) layout: FrameLayout,
    pub(crate) decoded: Option<&'a DecodedCover>,
    pub(crate) fade: CoverFade,
    pub(crate) wash: CoverWash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    Plain(PathBuf),
    Vinyl(VinylCacheKey),
}

impl Identity {
    fn changed_only_by_theme(&self, desired: &Self) -> bool {
        match (self, desired) {
            (Self::Vinyl(old), Self::Vinyl(new)) => key_changed_only_by_theme(old, new),
            (Self::Plain(_), _) | (Self::Vinyl(_), Self::Plain(_)) => false,
        }
    }
}

pub(crate) struct Built {
    pub(crate) pixmap: Arc<RgbaImage>,
    pub(crate) identity: Identity,
}

pub(crate) enum PixmapSource {
    Plain,
    Vinyl(Box<VinylCache>),
}

impl PixmapSource {
    fn build(&mut self, parts: VinylComposeParts<'_>) -> Option<Built> {
        match self {
            Self::Plain => plain_pixmap(parts.decoded),
            Self::Vinyl(cache) => {
                compose_vinyl(cache, parts).map(|(pixmap, key)| Built {
                    pixmap,
                    identity: Identity::Vinyl(key),
                })
            }
        }
    }

    fn holds_wash(&self, pixmap: &RgbaImage) -> bool {
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
struct WashState {
    wash: CoverWash,
    cell_width_px: u16,
}

impl WashState {
    const IDLE: Self = Self {
        wash: CoverWash::Idle,
        cell_width_px: 1,
    };
}

#[derive(Debug, Clone, Copy)]
struct Tick {
    now: Duration,
    wash: WashState,
}

struct Painted {
    protocol: StatefulProtocol,
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

struct Rebuild {
    plan: PaintPlan,
    built: Built,
    rect: Rect,
    fade: CoverFade,
    tick: Tick,
}

pub(crate) struct Cover {
    source: PixmapSource,
    painted: Option<Painted>,
    pixmap: Option<Arc<RgbaImage>>,
    crossfade: CoverCrossfade,
    wash: Option<Arc<RgbaImage>>,
}

impl fmt::Debug for Cover {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cover")
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

impl Cover {
    pub(crate) fn new(source: PixmapSource) -> Self {
        Self {
            source,
            painted: None,
            pixmap: None,
            crossfade: CoverCrossfade::default(),
            wash: None,
        }
    }

    pub(crate) fn discard_protocol(&mut self) {
        self.painted = None;
        self.pixmap = None;
        self.wash = None;
        self.crossfade = CoverCrossfade::default();
    }

    pub(crate) fn refresh(
        &mut self,
        picker: &Picker,
        refresh: CoverRefresh<'_>,
    ) -> OwnedCoverArt {
        let CoverRefresh {
            key,
            colors,
            clock,
            animations,
            layout,
            decoded,
            fade,
            wash,
        } = refresh;
        let Some(rect) = layout.cover else {
            self.discard_protocol();
            return OwnedCoverArt::Missing;
        };
        let font_size = picker.font_size();
        let Some(built) = self.source.build(VinylComposeParts {
            key,
            colors,
            decoded,
            rect,
            font_size,
        }) else {
            self.discard_protocol();
            return OwnedCoverArt::Missing;
        };
        let tick = Tick {
            now: clock,
            wash: WashState {
                wash,
                cell_width_px: font_size.width,
            },
        };
        let desired = Placed {
            identity: &built.identity,
            rect,
        };
        let plan = plan_paint(self.painted.as_ref().map(Painted::placed), desired);
        if plan == PaintPlan::Reuse {
            self.advance(picker, tick);
            return OwnedCoverArt::Image;
        }
        let fade = if animations == Animations::On && plan == PaintPlan::NewContent {
            fade
        } else {
            CoverFade::Withheld
        };
        self.rebuild(
            picker,
            Rebuild {
                plan,
                built,
                rect,
                fade,
                tick,
            },
        );
        OwnedCoverArt::Image
    }

    pub(crate) fn protocol_mut(&mut self) -> Option<&mut StatefulProtocol> {
        self.painted.as_mut().map(|painted| &mut painted.protocol)
    }

    pub(crate) fn motion(&self, now: Duration) -> CoverMotion {
        match (self.crossfade.stage(now), self.wash.is_some()) {
            (CrossfadeStage::Idle, false) => CoverMotion::Still,
            (CrossfadeStage::Idle, true)
            | (CrossfadeStage::Running | CrossfadeStage::Over, _) => {
                CoverMotion::Crossfading
            }
        }
    }

    fn rebuild(&mut self, picker: &Picker, input: Rebuild) {
        let Rebuild {
            plan,
            built,
            rect,
            fade,
            tick,
        } = input;
        let outgoing = self.pixmap.replace(built.pixmap);
        self.wash = None;
        let shown = if plan == PaintPlan::ThemeWash {
            if matches!(tick.wash.wash, CoverWash::Running { .. }) {
                self.wash = outgoing;
            }
            tick
        } else {
            if fade == CoverFade::Allowed
                && let Some(outgoing) = outgoing
            {
                self.crossfade.begin(outgoing, tick.now);
            }
            Tick {
                wash: WashState::IDLE,
                ..tick
            }
        };
        self.repaint(
            picker,
            PaintTarget {
                identity: built.identity,
                rect,
                tick: shown,
            },
        );
        self.hold(tick.wash.wash);
    }

    fn hold(&mut self, wash: CoverWash) {
        if matches!(wash, CoverWash::Running { .. })
            && self.wash.is_none()
            && let Some(pixmap) = self.pixmap.as_ref()
            && self.source.holds_wash(pixmap)
        {
            self.wash = Some(Arc::clone(pixmap));
        }
    }

    fn advance(&mut self, picker: &Picker, tick: Tick) {
        self.hold(tick.wash.wash);
        let released = self.wash.is_some() && tick.wash.wash == CoverWash::Idle;
        if released {
            self.wash = None;
        }
        let stage = self.crossfade.stage(tick.now);
        if stage == CrossfadeStage::Over {
            self.crossfade.finish_if_done(tick.now);
        }
        if released || stage != CrossfadeStage::Idle || self.wash.is_some() {
            self.repaint_current(picker, tick);
        }
    }

    fn repaint_current(&mut self, picker: &Picker, tick: Tick) {
        let Some(painted) = self.painted.as_ref() else {
            return;
        };
        let target = PaintTarget {
            identity: painted.identity.clone(),
            rect: painted.rect,
            tick,
        };
        self.repaint(picker, target);
    }

    fn repaint(&mut self, picker: &Picker, target: PaintTarget) {
        let Some(pixmap) = self.pixmap.as_ref() else {
            return;
        };
        let image = self
            .crossfade
            .crossfade_at(pixmap, target.tick.now)
            .or_else(|| self.washed(pixmap, &target))
            .unwrap_or_else(|| RgbaImage::clone(pixmap));
        let image = match self.source {
            PixmapSource::Plain => fit_to_rect(image, target.rect, picker.font_size()),
            PixmapSource::Vinyl(_) => image,
        };
        let protocol = cover_protocol(picker, DynamicImage::ImageRgba8(image));
        self.painted = Some(Painted {
            protocol,
            identity: target.identity,
            rect: target.rect,
        });
    }

    fn washed(&self, pixmap: &RgbaImage, target: &PaintTarget) -> Option<RgbaImage> {
        let CoverWash::Running {
            progress,
            screen_width,
        } = target.tick.wash.wash
        else {
            return None;
        };
        Some(wash_frame(WashFrame {
            old: self.wash.as_deref()?,
            new: pixmap,
            rect: target.rect,
            cell_width_px: target.tick.wash.cell_width_px,
            progress,
            screen_width,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use config::Animations;
    use image::{Rgba, RgbaImage};
    use kernel::domain::Revision;
    use raster::{SleeveFace, VinylCacheKey, VinylColors};
    use ratatui::layout::Rect;
    use ratatui_image::picker::Picker;
    use rstest::rstest;
    use widgets::{Breakpoint, FrameLayout};

    use crate::pixels::cover::{
        CoverFade,
        CoverKey,
        CoverWash,
        DecodedCover,
        lifecycle::{
            Cover,
            CoverRefresh,
            Identity,
            PaintPlan,
            PixmapSource,
            Placed,
            plan_paint,
        },
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

    fn vinyl_key(path: &str, theme_generation: Revision) -> VinylCacheKey {
        VinylCacheKey {
            config_generation: Revision::default(),
            theme_generation,
            path: Some(PathBuf::from(path)),
            face: SleeveFace::Art,
            size_px: 128,
        }
    }

    fn vinyl(path: &str) -> Identity {
        Identity::Vinyl(vinyl_key(path, Revision::default()))
    }

    fn vinyl_with_theme(path: &str, theme_generation: Revision) -> Identity {
        Identity::Vinyl(vinyl_key(path, theme_generation))
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
    #[case::vinyl_only_the_theme_generation_moved(
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

    fn refresh_parts(
        decoded: Option<&DecodedCover>,
        fade: CoverFade,
    ) -> CoverRefresh<'_> {
        CoverRefresh {
            key: CoverKey {
                config_generation: Revision::default(),
                theme_generation: Revision::default(),
            },
            colors: VinylColors::default(),
            clock: Duration::ZERO,
            animations: Animations::On,
            layout: layout_with_cover(rect()),
            decoded,
            fade,
            wash: CoverWash::Idle,
        }
    }

    #[test]
    fn a_crossfade_keeps_the_incoming_image_shared() {
        let picker = Picker::halfblocks();
        let mut cover = Cover::new(PixmapSource::Plain);
        let first = DecodedCover {
            path: PathBuf::from("a.jpg"),
            image: Arc::new(source_pixmap()),
        };
        cover.refresh(&picker, refresh_parts(Some(&first), CoverFade::Allowed));
        let second = DecodedCover {
            path: PathBuf::from("b.jpg"),
            image: Arc::new(source_pixmap()),
        };
        cover.refresh(&picker, refresh_parts(Some(&second), CoverFade::Allowed));
        let incoming = cover.pixmap.as_ref().expect("a pixmap after install");
        assert!(Arc::ptr_eq(incoming, &second.image));
    }

    #[test]
    fn a_settled_vinyl_refresh_shares_the_cached_pixmap() {
        let picker = Picker::halfblocks();
        let mut cover = Cover::new(PixmapSource::Vinyl(Box::default()));

        cover.refresh(&picker, refresh_parts(None, CoverFade::Allowed));
        let first = cover
            .pixmap
            .clone()
            .expect("a refresh with a cover rect paints a pixmap");

        cover.refresh(&picker, refresh_parts(None, CoverFade::Allowed));
        let second = cover
            .pixmap
            .clone()
            .expect("a settled second refresh keeps the painted pixmap");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
