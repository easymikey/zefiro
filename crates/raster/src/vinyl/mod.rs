use std::path::{Path, PathBuf};

use image::RgbaImage;
use kernel::domain::Revision;

use crate::memo::Memo;

mod art;
mod colors;
mod geometry;
mod layers;

pub(crate) use art::{VinylArt, prepare_art, sleeve_inset_side_px};
pub use colors::{SleeveFace, VinylColors};
use geometry::canvas_dims;
pub use geometry::{VinylLayout, canvas_aspect_ratio};
use layers::solid_fallback;
pub(crate) use layers::{
    VinylFrameBase,
    VinylFrameStyle,
    VinylSleeve,
    VinylSleeveOverlay,
    compose_vinyl_frame,
    prepare_frame_base,
    prepare_sleeve_overlay,
};

pub(crate) type VinylArtCacheKey = (Option<PathBuf>, u32);

pub(crate) type VinylBaseCacheKey = (Revision, Revision, u32);

pub(crate) type VinylOverlayCacheKey =
    (Revision, Revision, Option<PathBuf>, SleeveFace, u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VinylCacheKey {
    pub config_generation: Revision,
    pub theme_generation: Revision,
    pub path: Option<PathBuf>,
    pub face: SleeveFace,
    pub size_px: u32,
}

#[derive(Debug, Default)]
pub struct VinylCache {
    art: Memo<VinylArtCacheKey, Option<VinylArt>>,
    base: Memo<VinylBaseCacheKey, Option<VinylFrameBase>>,
    overlay: Memo<VinylOverlayCacheKey, Option<VinylSleeveOverlay>>,
    frame: Memo<VinylCacheKey, RgbaImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtCacheState {
    Cached,
    Missing,
}

impl VinylCache {
    #[must_use]
    pub fn art_cache_state(&self, path: Option<&Path>, size_px: u32) -> ArtCacheState {
        let key = (path.map(Path::to_path_buf), size_px.max(1));
        match self.art.get(&key) {
            Some(_) => ArtCacheState::Cached,
            None => ArtCacheState::Missing,
        }
    }
}

#[derive(Debug)]
pub struct DecodedArt {
    pub side_px: u32,
    pub image: Option<image::DynamicImage>,
}

#[derive(Debug, Default)]
pub struct VinylArtSource {
    pub path: Option<PathBuf>,
    pub decoded: Option<DecodedArt>,
}

#[derive(Debug)]
pub struct VinylRequest<'a> {
    pub cache: &'a mut VinylCache,
    pub colors: VinylColors,
    pub art: VinylArtSource,
    pub size_px: u32,
    pub face: SleeveFace,
    pub config_generation: Revision,
    pub theme_generation: Revision,
}

#[derive(Debug)]
pub enum VinylImage<'a> {
    Ready {
        pixmap: &'a RgbaImage,
        key: VinylCacheKey,
    },
    ArtWanted {
        side_px: u32,
    },
}

enum PreparedArt {
    Ready(Option<VinylArt>),
    Wanted,
}

#[must_use]
pub fn compose<'a>(request: VinylRequest<'a>) -> VinylImage<'a> {
    let VinylRequest {
        cache,
        colors,
        art,
        size_px,
        face,
        config_generation,
        theme_generation,
    } = request;
    let style = VinylFrameStyle {
        size_px: size_px.max(1),
        colors,
        layout: VinylLayout::default(),
    };
    let key = VinylCacheKey {
        config_generation,
        theme_generation,
        path: art.path.clone(),
        face,
        size_px: style.size_px,
    };
    let art_key = (key.path.clone(), style.size_px);
    if cache.art.get(&art_key).is_none() {
        match prepared_art(art, style) {
            PreparedArt::Wanted => {
                return VinylImage::ArtWanted {
                    side_px: sleeve_inset_side_px(style.size_px, &style.layout),
                };
            }
            PreparedArt::Ready(prepared) => cache.art.insert(art_key, prepared),
        }
    }
    VinylImage::Ready {
        pixmap: frame_image(cache, style, key.clone()),
        key,
    }
}

fn prepared_art(art: VinylArtSource, style: VinylFrameStyle) -> PreparedArt {
    if art.path.is_none() {
        return PreparedArt::Ready(None);
    }
    let Some(decoded) = art.decoded else {
        return PreparedArt::Wanted;
    };
    if decoded.side_px != sleeve_inset_side_px(style.size_px, &style.layout) {
        return PreparedArt::Wanted;
    }
    PreparedArt::Ready(
        decoded.image.map(|image| {
            prepare_art(&image.into_rgba8(), style.size_px, &style.layout)
        }),
    )
}

fn frame_image(
    cache: &mut VinylCache,
    style: VinylFrameStyle,
    key: VinylCacheKey,
) -> &RgbaImage {
    let art = cache
        .art
        .get(&(key.path.clone(), style.size_px))
        .and_then(Option::as_ref);
    let base = cache
        .base
        .get_or_insert_with(
            (key.config_generation, key.theme_generation, style.size_px),
            || prepare_frame_base(style),
        )
        .as_ref();
    let sleeve = VinylSleeve {
        style,
        face: key.face,
        art,
    };
    let overlay = cache
        .overlay
        .get_or_insert_with(
            (
                key.config_generation,
                key.theme_generation,
                key.path.clone(),
                key.face,
                style.size_px,
            ),
            || prepare_sleeve_overlay(&sleeve),
        )
        .as_ref();
    let frame = VinylFrame {
        art,
        size_px: style.size_px,
        face: key.face,
        colors: style.colors,
        layout: style.layout,
    };
    cache
        .frame
        .get_or_insert_with(key, || match (base, overlay) {
            (Some(base), Some(overlay)) => compose_vinyl_frame(base, overlay, &frame),
            _ => vinyl_image(&frame),
        })
}

pub(crate) struct VinylFrame<'a> {
    pub(crate) art: Option<&'a VinylArt>,
    pub(crate) size_px: u32,
    pub(crate) face: SleeveFace,
    pub(crate) colors: VinylColors,
    pub(crate) layout: VinylLayout,
}

pub(crate) fn vinyl_image(input: &VinylFrame<'_>) -> RgbaImage {
    let style = VinylFrameStyle {
        size_px: input.size_px,
        colors: input.colors,
        layout: input.layout,
    };
    let dims = canvas_dims(&style);

    let Some(base) = prepare_frame_base(style) else {
        return solid_fallback(dims.width, dims.height);
    };
    let sleeve_input = VinylSleeve {
        style,
        face: input.face,
        art: input.art,
    };
    let Some(overlay) = prepare_sleeve_overlay(&sleeve_input) else {
        return solid_fallback(dims.width, dims.height);
    };
    compose_vinyl_frame(&base, &overlay, input)
}

#[cfg(test)]
pub(crate) mod fixtures {
    use image::RgbaImage;

    pub(crate) fn synthetic_art(size: u32) -> RgbaImage {
        RgbaImage::from_fn(size, size, |x, y| {
            image::Rgba([
                u8::try_from(x * 255 / size.max(1)).unwrap_or(u8::MAX),
                u8::try_from(y * 255 / size.max(1)).unwrap_or(u8::MAX),
                128,
                255,
            ])
        })
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::{
        numeric::dimension_f32,
        vinyl::{
            VinylFrame,
            art::{VinylArt, prepare_art},
            colors::{SleeveFace, VinylColors},
            fixtures::synthetic_art,
            geometry,
            geometry::{VinylLayout, canvas_aspect_ratio},
            vinyl_image,
        },
    };

    fn prepared_art(art: &RgbaImage, size_px: u32) -> VinylArt {
        prepare_art(art, size_px, &VinylLayout::default())
    }

    fn base_input(art: Option<&VinylArt>) -> VinylFrame<'_> {
        VinylFrame {
            art,
            size_px: 64,
            face: SleeveFace::Art,
            colors: VinylColors::default(),
            layout: VinylLayout::default(),
        }
    }

    fn expected_peek_px(size_px: u32, layout: &VinylLayout) -> u32 {
        let size = dimension_f32(size_px);
        let disc_diameter = layout.disc_fraction * size;
        let peek = layout.slide_fraction * disc_diameter;
        let shadow_margin =
            layout.shadow_offset * size * geometry::shadow_horizontal_reach_fraction();
        crate::numeric::floor_u32((peek + shadow_margin).ceil())
    }

    #[test]
    fn canvas_dims_are_size_px_plus_peek_wide_and_size_px_tall() {
        let art = synthetic_art(32);
        let prepared = prepared_art(&art, 64);
        let input = base_input(Some(&prepared));
        let image = vinyl_image(&input);
        let peek = expected_peek_px(64, &VinylLayout::default());
        assert_eq!(image.dimensions(), (64 + peek, 64));
    }

    #[test]
    fn canvas_aspect_ratio_matches_rendered_dims() {
        let layout = VinylLayout::default();
        let art = synthetic_art(32);
        let prepared = prepared_art(&art, 64);
        let input = base_input(Some(&prepared));
        let image = vinyl_image(&input);
        let (width, height) = image.dimensions();
        let rendered_ratio = f64::from(width) / f64::from(height);
        assert!(
            (rendered_ratio - f64::from(canvas_aspect_ratio(&layout))).abs() < 0.02
        );
    }

    #[test]
    fn blank_and_art_render_differently() {
        let art = synthetic_art(32);
        let prepared = prepared_art(&art, 64);
        let mut blank = base_input(Some(&prepared));
        blank.face = SleeveFace::Blank;
        let mut with_art = base_input(Some(&prepared));
        with_art.face = SleeveFace::Art;

        assert_ne!(
            vinyl_image(&blank).into_raw(),
            vinyl_image(&with_art).into_raw()
        );
    }

    #[test]
    fn blank_and_art_share_the_sleeve_frame_but_differ_inside() {
        let size_px = 256;
        let art = synthetic_art(128);
        let prepared = prepared_art(&art, size_px);
        let mut blank = base_input(Some(&prepared));
        blank.size_px = size_px;
        blank.face = SleeveFace::Blank;
        let mut with_art = base_input(Some(&prepared));
        with_art.size_px = size_px;
        with_art.face = SleeveFace::Art;

        let blank_image = vinyl_image(&blank);
        let art_image = vinyl_image(&with_art);

        let layout = VinylLayout::default();
        let size = dimension_f32(size_px);
        let ring_x = crate::numeric::round_u32(
            (layout.border_width + layout.sleeve_padding) / 2.0 * size,
        );
        let mid = size_px / 2;
        for (x, y) in [(ring_x, mid), (mid, ring_x)] {
            assert_eq!(
                blank_image.get_pixel(x, y),
                art_image.get_pixel(x, y),
                "sleeve ring pixel at ({x}, {y}) should match between faces"
            );
        }

        assert_ne!(
            blank_image.get_pixel(mid, mid),
            art_image.get_pixel(mid, mid),
            "sleeve interior should differ between faces"
        );
    }

    #[test]
    fn same_input_renders_identical_bytes() {
        let art = synthetic_art(32);
        let prepared = prepared_art(&art, 64);
        let input = base_input(Some(&prepared));

        assert_eq!(
            vinyl_image(&input).into_raw(),
            vinyl_image(&input).into_raw()
        );
    }

    #[test]
    fn no_cover_renders_without_panicking() {
        for face in [SleeveFace::Blank, SleeveFace::Art] {
            let mut input = base_input(None);
            input.face = face;
            let image = vinyl_image(&input);
            let peek = expected_peek_px(64, &VinylLayout::default());
            assert_eq!(image.dimensions(), (64 + peek, 64));
        }
    }

    #[test]
    fn blank_face_ignores_a_present_cover() {
        let art = synthetic_art(32);
        let prepared = prepared_art(&art, 64);
        let mut blank_with_cover = base_input(Some(&prepared));
        blank_with_cover.face = SleeveFace::Blank;
        let mut blank_without_cover = base_input(None);
        blank_without_cover.face = SleeveFace::Blank;

        assert_eq!(
            vinyl_image(&blank_with_cover).into_raw(),
            vinyl_image(&blank_without_cover).into_raw()
        );
    }
}

#[cfg(test)]
mod compose_tests {
    use std::path::PathBuf;

    use kernel::domain::Revision;

    use crate::vinyl::{
        DecodedArt,
        SleeveFace,
        VinylArtSource,
        VinylCache,
        VinylColors,
        VinylImage,
        VinylLayout,
        VinylRequest,
        fixtures::synthetic_art,
        sleeve_inset_side_px,
    };

    const SIZE_PX: u32 = 96;

    fn request(cache: &mut VinylCache, art: VinylArtSource) -> VinylRequest<'_> {
        VinylRequest {
            cache,
            colors: VinylColors::default(),
            art,
            size_px: SIZE_PX,
            face: SleeveFace::Art,
            config_generation: Revision::default(),
            theme_generation: Revision::default(),
        }
    }

    fn decoded_art() -> DecodedArt {
        let side_px = sleeve_inset_side_px(SIZE_PX, &VinylLayout::default());
        DecodedArt {
            side_px,
            image: Some(image::DynamicImage::ImageRgba8(synthetic_art(side_px))),
        }
    }

    #[test]
    fn a_track_whose_cover_is_not_decoded_yet_asks_for_the_sleeve_side() {
        let mut cache = VinylCache::default();
        let art = VinylArtSource {
            path: Some(PathBuf::from("/music/a.flac")),
            decoded: None,
        };

        match crate::vinyl::compose(request(&mut cache, art)) {
            VinylImage::ArtWanted { side_px } => {
                assert_eq!(
                    side_px,
                    sleeve_inset_side_px(SIZE_PX, &VinylLayout::default())
                );
            }
            VinylImage::Ready { .. } => {
                panic!("undecoded art must not compose a frame")
            }
        }
    }

    #[test]
    fn a_cover_decoded_at_the_wrong_side_is_asked_for_again() {
        let mut cache = VinylCache::default();
        let art = VinylArtSource {
            path: Some(PathBuf::from("/music/a.flac")),
            decoded: Some(DecodedArt {
                side_px: 3,
                image: Some(image::DynamicImage::ImageRgba8(synthetic_art(3))),
            }),
        };

        assert!(matches!(
            crate::vinyl::compose(request(&mut cache, art)),
            VinylImage::ArtWanted { .. }
        ));
    }

    #[test]
    fn a_decoded_cover_composes_a_frame_under_the_key_it_was_built_with() {
        let mut cache = VinylCache::default();
        let path = PathBuf::from("/music/a.flac");
        let art = VinylArtSource {
            path: Some(path.clone()),
            decoded: Some(decoded_art()),
        };

        let pixmap = {
            let VinylImage::Ready { pixmap, key } =
                crate::vinyl::compose(request(&mut cache, art))
            else {
                panic!("a decoded cover must compose a frame");
            };
            assert_eq!(key.path.as_ref(), Some(&path));
            assert_eq!(key.size_px, SIZE_PX);
            assert_eq!(pixmap.height(), SIZE_PX);
            pixmap.clone()
        };

        let again = VinylArtSource {
            path: Some(path),
            decoded: None,
        };
        let VinylImage::Ready { pixmap: repeat, .. } =
            crate::vinyl::compose(request(&mut cache, again))
        else {
            panic!("the remembered art must compose again without a fresh decode");
        };
        assert_eq!(pixmap.into_raw(), repeat.clone().into_raw());
    }

    #[test]
    fn a_silent_player_with_no_track_composes_a_frame_without_art() {
        let mut cache = VinylCache::default();
        let art = VinylArtSource::default();

        let VinylImage::Ready { pixmap, key } =
            crate::vinyl::compose(request(&mut cache, art))
        else {
            panic!("no track still composes the empty sleeve");
        };
        assert_eq!(key.path, None);
        assert_eq!(pixmap.height(), SIZE_PX);
    }
}

#[cfg(test)]
mod vinyl_cache_tests {
    use std::cell::Cell;

    use kernel::domain::Revision;

    use crate::vinyl::{VinylBaseCacheKey, VinylCache, layers::VinylFrameBase};

    fn revision(bumps: u64) -> Revision {
        (0..bumps).fold(Revision::default(), |revision, _| revision.next())
    }

    #[test]
    fn base_reused_when_neither_generation_moves_rebuilt_when_theme_generation_does() {
        let mut cache = VinylCache::default();
        let calls = Cell::new(0u32);
        let build = || {
            calls.set(calls.get() + 1);
            None::<VinylFrameBase>
        };
        let key: VinylBaseCacheKey = (revision(1), revision(1), 128);

        cache.base.get_or_insert_with(key, build);
        cache.base.get_or_insert_with(key, build);
        assert_eq!(calls.get(), 1, "unchanged key must not rebuild");

        let theme_moved: VinylBaseCacheKey = (revision(1), revision(2), 128);
        cache.base.get_or_insert_with(theme_moved, build);
        assert_eq!(
            calls.get(),
            2,
            "a moved theme_generation must force a rebuild"
        );

        let config_moved: VinylBaseCacheKey = (revision(2), revision(2), 128);
        cache.base.get_or_insert_with(config_moved, build);
        assert_eq!(
            calls.get(),
            3,
            "a moved config_generation must also force a rebuild"
        );
    }
}
