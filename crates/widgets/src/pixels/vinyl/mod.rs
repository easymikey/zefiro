use std::path::PathBuf;

use config::Rgb;
use image::RgbaImage;
use kernel::domain::Revision;
use tiny_skia::Pixmap;

use crate::{ActiveTheme, Role, shade};

mod art;
mod geometry;
mod layers;

pub(crate) use art::{VinylArt, prepare_art};
use geometry::VinylGeometry;
pub(crate) use geometry::canvas_aspect_ratio;
use layers::solid_fallback;
pub(crate) use layers::{
    VinylFrameStyle,
    VinylParts,
    compose_vinyl_frame,
    paint_record_layer,
    paint_sleeve_layer,
};

const RECORD_SHADE_FACTOR: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VinylStyle {
    pub paper: Rgb,
    pub border: Rgb,
    pub record: Rgb,
    pub groove: Rgb,
    pub accent: Rgb,
    pub shadow: Rgb,
}

impl VinylStyle {
    #[must_use]
    pub fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let colors = &theme.colors;
        Self {
            paper: colors.role(Role::Text),
            border: colors.role(Role::Frame),
            record: shade(colors.role(Role::Dim), RECORD_SHADE_FACTOR),
            groove: Rgb([0xff, 0xff, 0xff]),
            accent: colors.role(Role::Accent),
            shadow: Rgb([0x00, 0x00, 0x00]),
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            paper: Rgb([0xec, 0xe6, 0xd6]),
            border: Rgb([0x3a, 0x3a, 0x3a]),
            record: Rgb([0x10, 0x10, 0x10]),
            groove: Rgb([0xff, 0xff, 0xff]),
            accent: Rgb([0xff, 0x6b, 0x3d]),
            shadow: Rgb([0x00, 0x00, 0x00]),
        }
    }
}

#[derive(Debug)]
struct Memo<K, V> {
    key: Option<K>,
    remembered: Option<V>,
}

impl<K: PartialEq, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq, V> Memo<K, V> {
    #[must_use]
    const fn new() -> Self {
        Self {
            key: None,
            remembered: None,
        }
    }

    fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> &V {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.remembered = None;
        }
        &*self.remembered.get_or_insert_with(f)
    }
}

pub(crate) type VinylArtCacheKey = (Option<PathBuf>, u32);

pub(crate) type VinylBaseCacheKey = (Revision, Revision, u32);

pub(crate) type VinylOverlayCacheKey = (Revision, Revision, Option<PathBuf>, u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VinylCacheKey {
    pub config_revision: Revision,
    pub theme_revision: Revision,
    pub path: Option<PathBuf>,
    pub size_px: u32,
    pub colors: VinylStyle,
}

#[derive(Debug, Default)]
pub struct VinylCache {
    art: Memo<VinylArtCacheKey, Option<VinylArt>>,
    record: Memo<VinylBaseCacheKey, Option<Pixmap>>,
    sleeve: Memo<VinylOverlayCacheKey, Option<Pixmap>>,
    frame: Memo<VinylCacheKey, RgbaImage>,
}

impl VinylCache {
    #[must_use]
    pub fn compose(
        &mut self,
        key: VinylCacheKey,
        art: Option<&RgbaImage>,
    ) -> &RgbaImage {
        let style = VinylFrameStyle {
            size_px: key.size_px.max(1),
            colors: key.colors,
        };
        let geometry = VinylGeometry::new(style.size_px);
        let art = self
            .art
            .get_or_insert_with((key.path.clone(), style.size_px), || {
                art.map(|image| prepare_art(image, style.size_px))
            })
            .as_ref();
        let record = self
            .record
            .get_or_insert_with(
                (key.config_revision, key.theme_revision, style.size_px),
                || paint_record_layer(style),
            )
            .as_ref();
        let parts = VinylParts { style, art };
        let sleeve = self
            .sleeve
            .get_or_insert_with(
                (
                    key.config_revision,
                    key.theme_revision,
                    key.path.clone(),
                    style.size_px,
                ),
                || paint_sleeve_layer(&parts),
            )
            .as_ref();
        self.frame
            .get_or_insert_with(key, || match (record, sleeve) {
                (Some(record), Some(sleeve)) => {
                    compose_vinyl_frame(record, sleeve, &parts)
                }
                _ => solid_fallback(geometry.width_px, geometry.height_px),
            })
    }
}

#[cfg(test)]
pub(crate) fn compose_uncached(parts: &VinylParts<'_>) -> RgbaImage {
    let geometry = VinylGeometry::new(parts.style.size_px);

    let Some(record) = paint_record_layer(parts.style) else {
        return solid_fallback(geometry.width_px, geometry.height_px);
    };
    let Some(sleeve) = paint_sleeve_layer(parts) else {
        return solid_fallback(geometry.width_px, geometry.height_px);
    };
    compose_vinyl_frame(&record, &sleeve, parts)
}

#[cfg(test)]
pub(crate) mod test_support {
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
    use std::{cell::Cell, path::PathBuf};

    use kernel::domain::Revision;
    use tiny_skia::Pixmap;

    use crate::pixels::{
        numeric::dimension_f32,
        vinyl::{
            Memo,
            VinylArt,
            VinylBaseCacheKey,
            VinylCache,
            VinylCacheKey,
            VinylFrameStyle,
            VinylParts,
            VinylStyle,
            compose_uncached,
            geometry,
            geometry::{VINYL_LAYOUT, canvas_aspect_ratio},
            prepare_art,
            test_support::synthetic_art,
        },
    };

    const SIZE_PX: u32 = 96;

    fn parts_for(art: Option<&VinylArt>) -> VinylParts<'_> {
        VinylParts {
            style: VinylFrameStyle {
                size_px: 64,
                colors: VinylStyle::fixture(),
            },
            art,
        }
    }

    fn expected_peek_px(size_px: u32) -> u32 {
        let size = dimension_f32(size_px);
        let disc_diameter = VINYL_LAYOUT.disc_fraction * size;
        let peek = VINYL_LAYOUT.slide_fraction * disc_diameter;
        let shadow_margin = VINYL_LAYOUT.shadow_offset
            * size
            * geometry::shadow_horizontal_reach_fraction();
        crate::pixels::numeric::floor::<u32>((peek + shadow_margin).ceil())
    }

    #[test]
    fn canvas_size_is_size_px_plus_peek_wide_and_size_px_tall() {
        let art = synthetic_art(32);
        let prepared = prepare_art(&art, 64);
        let parts = parts_for(Some(&prepared));
        let image = compose_uncached(&parts);
        let peek = expected_peek_px(64);
        assert_eq!(image.dimensions(), (64 + peek, 64));
    }

    #[test]
    fn canvas_aspect_ratio_matches_rendered_size() {
        let art = synthetic_art(32);
        let prepared = prepare_art(&art, 64);
        let parts = parts_for(Some(&prepared));
        let image = compose_uncached(&parts);
        let (width, height) = image.dimensions();
        let rendered_ratio = f64::from(width) / f64::from(height);
        assert!((rendered_ratio - f64::from(canvas_aspect_ratio())).abs() < 0.02);
    }

    #[test]
    fn same_input_renders_identical_bytes() {
        let art = synthetic_art(32);
        let prepared = prepare_art(&art, 64);
        let parts = parts_for(Some(&prepared));

        assert_eq!(
            compose_uncached(&parts).into_raw(),
            compose_uncached(&parts).into_raw()
        );
    }

    #[test]
    fn no_cover_renders_without_panicking() {
        let image = compose_uncached(&parts_for(None));
        let peek = expected_peek_px(64);
        assert_eq!(image.dimensions(), (64 + peek, 64));
    }

    fn key(path: Option<PathBuf>, theme_revision: Revision) -> VinylCacheKey {
        VinylCacheKey {
            config_revision: Revision::default(),
            theme_revision,
            path,
            size_px: SIZE_PX,
            colors: VinylStyle::fixture(),
        }
    }

    #[test]
    fn a_cover_changes_the_frame_and_keeps_the_sleeve_height() {
        let mut cache = VinylCache::default();
        let art = synthetic_art(SIZE_PX);
        let path = Some(PathBuf::from("/music/a.flac"));

        let with_art = cache
            .compose(key(path, Revision::default()), Some(&art))
            .clone();
        let without = cache.compose(key(None, Revision::default()), None).clone();

        assert_eq!(with_art.height(), SIZE_PX);
        assert_ne!(with_art.into_raw(), without.into_raw());
    }

    #[test]
    fn a_rebuilt_frame_reuses_the_remembered_art() {
        let mut cache = VinylCache::default();
        let art = synthetic_art(SIZE_PX);
        let path = Some(PathBuf::from("/music/a.flac"));

        let first = cache
            .compose(key(path.clone(), Revision::default()), Some(&art))
            .clone();
        let themed = Revision::default().next();
        let again = cache.compose(key(path, themed), None).clone();

        assert_eq!(first.into_raw(), again.into_raw());
    }

    #[test]
    fn same_key_does_not_recompute() {
        let mut c: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        for _ in 0..3 {
            c.get_or_insert_with(7, || {
                calls += 1;
                42
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(
            *c.get_or_insert_with(7, || panic!("key 7 is already cached")),
            42
        );
    }

    #[test]
    fn different_key_recomputes() {
        let mut cache: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        let mut val = |memo: &mut Memo<u32, u32>, k: u32| -> u32 {
            *memo.get_or_insert_with(k, || {
                calls += 1;
                k * 2
            })
        };
        assert_eq!(val(&mut cache, 1), 2);
        assert_eq!(val(&mut cache, 2), 4);
        assert_eq!(val(&mut cache, 2), 4);
        assert_eq!(calls, 2);
    }

    #[test]
    fn a_second_value_for_the_same_key_is_ignored() {
        let mut c: Memo<u32, &'static str> = Memo::new();
        c.get_or_insert_with(5, || "first");
        assert_eq!(*c.get_or_insert_with(5, || "second"), "first");
    }

    fn revision(bumps: u64) -> Revision {
        (0..bumps).fold(Revision::default(), |revision, _| revision.next())
    }

    #[test]
    fn the_base_is_rebuilt_only_when_a_revision_moves() {
        let mut cache = VinylCache::default();
        let calls = Cell::new(0u32);
        let build = || {
            calls.set(calls.get() + 1);
            None::<Pixmap>
        };
        let key: VinylBaseCacheKey = (revision(1), revision(1), 128);

        cache.record.get_or_insert_with(key, build);
        cache.record.get_or_insert_with(key, build);
        assert_eq!(calls.get(), 1, "unchanged key must not rebuild");

        let theme_moved: VinylBaseCacheKey = (revision(1), revision(2), 128);
        cache.record.get_or_insert_with(theme_moved, build);
        assert_eq!(
            calls.get(),
            2,
            "a moved theme_revision must force a rebuild"
        );

        let config_moved: VinylBaseCacheKey = (revision(2), revision(2), 128);
        cache.record.get_or_insert_with(config_moved, build);
        assert_eq!(
            calls.get(),
            3,
            "a moved config_revision must also force a rebuild"
        );
    }
}
