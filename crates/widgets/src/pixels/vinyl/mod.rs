use std::path::PathBuf;

use image::RgbaImage;
use kernel::domain::{appearance::Rgb, geometry::Pixels};
use tiny_skia::Pixmap;

use crate::theme::{active_theme::ActiveTheme, rgb::shade};

pub(crate) mod art;
pub(crate) mod geometry;
pub(crate) mod layers;

use art::{VinylArt, prepare_art};
use geometry::VinylGeometry;
use layers::{
    SleeveInput,
    VinylFrame,
    compose_vinyl_frame,
    paint_record_layer,
    paint_sleeve_layer,
    solid_fallback,
};

const RECORD_SHADE_FACTOR: f32 = 0.35;
const GROOVE_COLOR: Rgb = Rgb([0xff, 0xff, 0xff]);
const SHADOW_COLOR: Rgb = Rgb([0x00, 0x00, 0x00]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VinylStyle {
    pub(crate) paper: Rgb,
    pub(crate) border: Rgb,
    pub(crate) record: Rgb,
    pub(crate) groove: Rgb,
    pub(crate) accent: Rgb,
    pub(crate) shadow: Rgb,
}

impl VinylStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let colors = &theme.colors;
        Self {
            paper: colors.foreground,
            border: colors.muted_foreground,
            record: shade(colors.muted_foreground, RECORD_SHADE_FACTOR),
            groove: GROOVE_COLOR,
            accent: colors.accent,
            shadow: SHADOW_COLOR,
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
    entry: Option<(K, V)>,
}

impl<K: PartialEq, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq, V> Memo<K, V> {
    #[must_use]
    const fn new() -> Self {
        Self { entry: None }
    }

    fn cached_or_painted(&mut self, key: K, f: impl FnOnce() -> V) -> &V {
        let entry = self
            .entry
            .take()
            .filter(|(remembered, _)| *remembered == key)
            .unwrap_or_else(|| (key, f()));
        &self.entry.insert(entry).1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VinylCacheKey {
    pub(crate) path: Option<PathBuf>,
    pub(crate) side: Pixels,
    pub(crate) vinyl_style: VinylStyle,
}

#[derive(Debug, Default)]
pub struct VinylCache {
    art: Memo<(Option<PathBuf>, Pixels), Option<VinylArt>>,
    record: Memo<(VinylStyle, Pixels), Option<Pixmap>>,
    sleeve: Memo<(VinylStyle, Option<PathBuf>, Pixels), Option<Pixmap>>,
}

impl VinylCache {
    #[must_use]
    pub(crate) fn compose(
        &mut self,
        key: &VinylCacheKey,
        image: Option<&RgbaImage>,
    ) -> RgbaImage {
        let frame = VinylFrame {
            side: Pixels(key.side.0.max(1)),
            style: key.vinyl_style,
        };
        let canvas_side = frame.side;
        let geometry = VinylGeometry::new(canvas_side);
        let art = self
            .art
            .cached_or_painted((key.path.clone(), canvas_side), || {
                image.map(|image| prepare_art(image, canvas_side))
            })
            .as_ref();
        let record = self
            .record
            .cached_or_painted((key.vinyl_style, canvas_side), || {
                paint_record_layer(frame)
            })
            .as_ref();
        let sleeve_input = SleeveInput { frame, art };
        let sleeve = self
            .sleeve
            .cached_or_painted((key.vinyl_style, key.path.clone(), canvas_side), || {
                paint_sleeve_layer(&sleeve_input)
            })
            .as_ref();
        match (record, sleeve) {
            (Some(record), Some(sleeve)) => {
                compose_vinyl_frame(record, sleeve, &sleeve_input)
            }
            _ => solid_fallback(geometry.width.0, geometry.height.0),
        }
    }
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

    use kernel::domain::{appearance::Rgb, geometry::Pixels};
    use tiny_skia::Pixmap;

    use crate::pixels::{
        numeric::dimension_f32,
        vinyl::{
            Memo,
            VinylCache,
            VinylCacheKey,
            VinylStyle,
            geometry,
            geometry::{VINYL_LAYOUT, canvas_aspect_ratio},
            test_support::synthetic_art,
        },
    };

    const CANVAS_SIDE: Pixels = Pixels(96);

    fn composed(art: Option<&image::RgbaImage>) -> image::RgbaImage {
        VinylCache::default().compose(&key(None), art)
    }

    fn expected_peek(canvas_side: Pixels) -> u32 {
        let size = dimension_f32(canvas_side.0);
        let disc_diameter = VINYL_LAYOUT.disc_fraction * size;
        let peek = VINYL_LAYOUT.slide_fraction * disc_diameter;
        let shadow_margin = VINYL_LAYOUT.shadow_offset
            * size
            * geometry::shadow_horizontal_reach_fraction();
        crate::pixels::numeric::floor::<u32>((peek + shadow_margin).ceil())
    }

    #[test]
    fn canvas_width_is_the_canvas_side_plus_peek_and_height_is_the_canvas_side() {
        let image = composed(Some(&synthetic_art(32)));
        let peek = expected_peek(CANVAS_SIDE);
        assert_eq!(image.dimensions(), (CANVAS_SIDE.0 + peek, CANVAS_SIDE.0));
    }

    #[test]
    fn canvas_aspect_ratio_matches_rendered_size() {
        let image = composed(Some(&synthetic_art(32)));
        let (width, height) = image.dimensions();
        let rendered_ratio = f64::from(width) / f64::from(height);
        assert!((rendered_ratio - f64::from(canvas_aspect_ratio())).abs() < 0.02);
    }

    #[test]
    fn same_input_renders_identical_bytes() {
        let art = synthetic_art(32);

        assert_eq!(
            composed(Some(&art)).into_raw(),
            composed(Some(&art)).into_raw()
        );
    }

    #[test]
    fn no_cover_renders_without_panicking() {
        let image = composed(None);
        let peek = expected_peek(CANVAS_SIDE);
        assert_eq!(image.dimensions(), (CANVAS_SIDE.0 + peek, CANVAS_SIDE.0));
    }

    fn key(path: Option<PathBuf>) -> VinylCacheKey {
        VinylCacheKey {
            path,
            side: CANVAS_SIDE,
            vinyl_style: VinylStyle::fixture(),
        }
    }

    #[test]
    fn a_cover_changes_the_frame_and_keeps_the_sleeve_height() {
        let mut cache = VinylCache::default();
        let art = synthetic_art(CANVAS_SIDE.0);
        let path = Some(PathBuf::from("/music/a.flac"));

        let with_art = cache.compose(&key(path), Some(&art));
        let without = cache.compose(&key(None), None);

        assert_eq!(with_art.height(), CANVAS_SIDE.0);
        assert_ne!(with_art.into_raw(), without.into_raw());
    }

    #[test]
    fn a_rebuilt_frame_reuses_the_remembered_art() {
        let mut cache = VinylCache::default();
        let art = synthetic_art(CANVAS_SIDE.0);
        let path = Some(PathBuf::from("/music/a.flac"));

        let first = cache.compose(&key(path.clone()), Some(&art));
        let again = cache.compose(&key(path), None);

        assert_eq!(first.into_raw(), again.into_raw());
    }

    #[test]
    fn same_key_does_not_recompute() {
        let mut memo: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        for _ in 0..3 {
            memo.cached_or_painted(7, || {
                calls += 1;
                42
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(
            *memo.cached_or_painted(7, || panic!("key 7 is already cached")),
            42
        );
    }

    #[test]
    fn different_key_recomputes() {
        let mut memo: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        let mut doubled = |key: u32| -> u32 {
            *memo.cached_or_painted(key, || {
                calls += 1;
                key * 2
            })
        };
        assert_eq!(doubled(1), 2);
        assert_eq!(doubled(2), 4);
        assert_eq!(doubled(2), 4);
        assert_eq!(calls, 2);
    }

    #[test]
    fn a_second_value_for_the_same_key_is_ignored() {
        let mut memo: Memo<u32, &'static str> = Memo::new();
        memo.cached_or_painted(5, || "first");
        assert_eq!(*memo.cached_or_painted(5, || "second"), "first");
    }

    #[test]
    fn the_record_is_rebuilt_only_when_its_vinyl_style_or_side_move() {
        let mut cache = VinylCache::default();
        let calls = Cell::new(0u32);
        let build = || {
            calls.set(calls.get() + 1);
            None::<Pixmap>
        };
        let colors = VinylStyle::fixture();

        cache.record.cached_or_painted((colors, Pixels(128)), build);
        cache.record.cached_or_painted((colors, Pixels(128)), build);
        assert_eq!(calls.get(), 1, "unchanged colors and size must not rebuild");

        let recolored_style = VinylStyle {
            accent: Rgb([0x3d, 0x9b, 0xff]),
            ..colors
        };
        cache
            .record
            .cached_or_painted((recolored_style, Pixels(128)), build);
        assert_eq!(calls.get(), 2, "new colors must force a rebuild");

        cache
            .record
            .cached_or_painted((recolored_style, Pixels(256)), build);
        assert_eq!(calls.get(), 3, "a new size must also force a rebuild");
    }
}
