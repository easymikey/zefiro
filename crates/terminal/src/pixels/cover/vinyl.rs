use std::{path::Path, sync::Arc};

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
use ratatui_image::FontSize;
use widgets::{Cells, Pixels};

use crate::pixels::cover::{CoverKey, DecodedCover};

#[must_use]
pub(crate) fn key_changed_only_by_theme(
    old: &VinylCacheKey,
    new: &VinylCacheKey,
) -> bool {
    old.theme_generation != new.theme_generation
        && old.config_generation == new.config_generation
        && old.path == new.path
        && old.face == new.face
        && old.size_px == new.size_px
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct VinylComposeParts<'a> {
    pub(crate) key: CoverKey,
    pub(crate) colors: VinylColors,
    pub(crate) decoded: Option<&'a DecodedCover>,
    pub(crate) rect: Rect,
    pub(crate) font_size: FontSize,
}

#[must_use]
pub(crate) fn compose_vinyl(
    cache: &mut VinylCache,
    sources: VinylComposeParts<'_>,
) -> Option<(Arc<RgbaImage>, VinylCacheKey)> {
    let VinylComposeParts {
        key,
        colors,
        decoded,
        rect,
        font_size,
    } = sources;
    let size_px = vinyl_size_px(rect, font_size);
    let path = art_path(decoded);
    let art = VinylArtSource {
        path: path.map(Path::to_path_buf),
        decoded: decode_art(cache, decoded, size_px),
    };
    let request = VinylRequest {
        cache,
        colors,
        art,
        size_px,
        face: SleeveFace::Art,
        config_generation: key.config_generation,
        theme_generation: key.theme_generation,
    };
    let VinylImage::Ready {
        pixmap,
        key: cache_key,
    } = compose(request)
    else {
        return None;
    };
    Some((Arc::new(pixmap.clone()), cache_key))
}

#[must_use]
fn art_path(decoded: Option<&DecodedCover>) -> Option<&Path> {
    decoded.map(|cover| cover.path.as_path())
}

#[must_use]
fn decode_art(
    cache: &VinylCache,
    decoded: Option<&DecodedCover>,
    size_px: u32,
) -> Option<DecodedArt> {
    let cover = decoded?;
    let state = cache.art_cache_state(Some(&cover.path), size_px);
    if state == ArtCacheState::Cached {
        return None;
    }
    Some(DecodedArt {
        side_px: sleeve_inset_side_px(size_px),
        image: Some(DynamicImage::ImageRgba8((*cover.image).clone())),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    use image::{Rgba, RgbaImage};
    use kernel::domain::Revision;
    use raster::{
        SleeveFace,
        VinylArtSource,
        VinylCache,
        VinylColors,
        VinylImage,
        VinylRequest,
        compose,
    };

    use crate::pixels::cover::{
        DecodedCover,
        vinyl::{art_path, decode_art, sleeve_inset_side_px},
    };

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
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))),
        };
        let size_px = 96;

        let first_decoded = decode_art(&cache, Some(&cover), size_px);
        assert!(
            first_decoded.is_some(),
            "an empty cache must build the art source"
        );

        let request = VinylRequest {
            cache: &mut cache,
            colors: VinylColors::default(),
            art: VinylArtSource {
                path: art_path(Some(&cover)).map(Path::to_path_buf),
                decoded: first_decoded,
            },
            size_px,
            face: SleeveFace::Art,
            config_generation: Revision::default(),
            theme_generation: Revision::default(),
        };
        assert!(matches!(compose(request), VinylImage::Ready { .. }));

        let second_decoded = decode_art(&cache, Some(&cover), size_px);
        assert!(
            second_decoded.is_none(),
            "a cached key must not rebuild the art source"
        );
    }
}
