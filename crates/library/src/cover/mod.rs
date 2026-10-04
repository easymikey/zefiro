use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use fast_image_resize::{
    PixelType,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::{DynamicImage, RgbaImage};
use kernel::{cmd::CoverJob, domain::geometry::Pixels};

use crate::tags::embedded_cover;

pub(crate) mod decoding;

#[derive(Debug)]
pub enum CoverArt {
    Image(Arc<RgbaImage>),
    Missing,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot decode embedded art: {source}")]
pub struct CoverError {
    pub path: PathBuf,
    #[source]
    pub source: image::ImageError,
}

#[derive(Debug)]
pub struct CoverDecoded {
    pub path: PathBuf,
    pub side: Pixels,
    pub art: CoverArt,
}

pub(crate) const CACHE_CAPACITY: usize = 8;

#[derive(Debug, PartialEq)]
struct CoverCacheEntry {
    path: PathBuf,
    side: Pixels,
    image: Option<Arc<RgbaImage>>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct CoverCache {
    entries: VecDeque<CoverCacheEntry>,
}

impl CoverCache {
    pub(crate) fn answer(&mut self, job: &CoverJob) -> Option<CoverDecoded> {
        let position = self
            .entries
            .iter()
            .position(|entry| entry.path == job.path && entry.side == job.side)?;
        let entry = self.entries.remove(position)?;
        let art = entry.image.as_ref().map_or(CoverArt::Missing, |image| {
            CoverArt::Image(Arc::clone(image))
        });
        self.entries.push_front(entry);
        Some(CoverDecoded {
            path: job.path.clone(),
            side: job.side,
            art,
        })
    }

    pub(crate) fn remember(&mut self, decoded: &CoverDecoded) {
        let image = match &decoded.art {
            CoverArt::Image(image) => Some(Arc::clone(image)),
            CoverArt::Missing => None,
        };
        self.entries
            .retain(|entry| entry.path != decoded.path || entry.side != decoded.side);
        if self.entries.len() == CACHE_CAPACITY {
            self.entries.pop_back();
        }
        self.entries.push_front(CoverCacheEntry {
            path: decoded.path.clone(),
            side: decoded.side,
            image,
        });
    }
}

pub(crate) fn decode(job: CoverJob) -> Result<CoverDecoded, CoverError> {
    let art = match embedded_cover(&job.path) {
        Ok(Some(bytes)) => decode_bytes(&bytes, job.side),
        Ok(None) => Ok(CoverArt::Missing),
        Err(error) => Err(image::ImageError::IoError(std::io::Error::other(error))),
    };
    match art {
        Ok(art) => Ok(CoverDecoded {
            path: job.path,
            side: job.side,
            art,
        }),
        Err(source) => Err(CoverError {
            path: job.path,
            source,
        }),
    }
}

fn decode_bytes(bytes: &[u8], side: Pixels) -> Result<CoverArt, image::ImageError> {
    let fitted = fit_square(image::load_from_memory(bytes)?, side.0)?;
    Ok(fitted.map_or(CoverArt::Missing, |image| CoverArt::Image(Arc::new(image))))
}

fn resize_failed(error: &impl std::fmt::Display) -> image::ImageError {
    image::ImageError::Parameter(image::error::ParameterError::from_kind(
        image::error::ParameterErrorKind::Generic(error.to_string()),
    ))
}

pub(crate) fn fit_square(
    image: DynamicImage,
    side: u32,
) -> Result<Option<RgbaImage>, image::ImageError> {
    let source = image.into_rgba8();
    let (source_width, source_height) = source.dimensions();
    if source_width == 0 || source_height == 0 {
        return Ok(None);
    }
    let side = side.max(1);
    let crop = source_width.min(source_height);
    let crop_x = (source_width - crop) / 2;
    let crop_y = (source_height - crop) / 2;
    let source_view = ImageRef::new(
        source_width,
        source_height,
        source.as_raw(),
        PixelType::U8x4,
    )
    .map_err(|error| resize_failed(&error))?;
    let mut target = Image::new(side, side, PixelType::U8x4);
    let options = ResizeOptions::new()
        .crop(
            f64::from(crop_x),
            f64::from(crop_y),
            f64::from(crop),
            f64::from(crop),
        )
        .use_alpha(false);
    Resizer::new()
        .resize(&source_view, &mut target, &options)
        .map_err(|error| resize_failed(&error))?;
    Ok(RgbaImage::from_raw(side, side, target.into_vec()))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use image::{DynamicImage, RgbaImage};
    use kernel::{cmd::CoverJob, domain::geometry::Pixels};

    use crate::cover::{
        CACHE_CAPACITY,
        CoverArt,
        CoverCache,
        CoverDecoded,
        decode,
        fit_square,
    };

    fn job(path: &str, side: u32) -> CoverJob {
        CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        }
    }

    #[test]
    fn fit_square_center_crops_and_resizes_to_the_requested_side() {
        let wide = DynamicImage::ImageRgba8(RgbaImage::new(300, 200));
        let side = 48;

        let fitted = fit_square(wide, side).unwrap();

        assert_eq!(
            fitted.map(|image| (image.width(), image.height())),
            Some((side, side))
        );
    }

    #[test]
    fn fit_square_treats_an_empty_decode_as_missing_art() {
        let empty = DynamicImage::ImageRgba8(RgbaImage::new(0, 0));

        assert!(fit_square(empty, 48).unwrap().is_none());
    }

    #[test]
    fn a_job_for_a_file_without_a_tag_decodes_as_missing_art() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("untagged.wav");
        std::fs::write(&path, include_bytes!("../../tests/fixtures/tone.wav")).unwrap();

        let decoded = decode(job(path.to_str().unwrap(), 64)).unwrap();

        assert!(matches!(decoded.art, CoverArt::Missing));
    }

    fn decoded(path: &str, art: CoverArt) -> CoverDecoded {
        CoverDecoded {
            path: PathBuf::from(path),
            side: Pixels(64),
            art,
        }
    }

    fn image() -> CoverArt {
        CoverArt::Image(Arc::new(RgbaImage::new(64, 64)))
    }

    #[test]
    fn a_remembered_cover_answers_the_same_job() {
        let mut cache = CoverCache::default();
        cache.remember(&decoded("/music/one.flac", image()));

        let answer = cache.answer(&job("/music/one.flac", 64));

        assert!(matches!(
            answer,
            Some(CoverDecoded {
                art: CoverArt::Image(_),
                ..
            })
        ));
        assert!(cache.answer(&job("/music/one.flac", 32)).is_none());
    }

    #[test]
    fn a_remembered_missing_cover_answers_missing() {
        let mut cache = CoverCache::default();
        cache.remember(&decoded("/music/one.flac", CoverArt::Missing));

        let answer = cache.answer(&job("/music/one.flac", 64));

        assert!(matches!(
            answer,
            Some(CoverDecoded {
                art: CoverArt::Missing,
                ..
            })
        ));
    }

    #[test]
    fn the_ninth_distinct_cover_evicts_the_oldest_cached_entry() {
        let mut cache = CoverCache::default();
        let paths: Vec<String> = (0..=CACHE_CAPACITY)
            .map(|index| format!("/music/{index}.flac"))
            .collect();
        for path in &paths {
            cache.remember(&decoded(path, image()));
        }

        assert!(cache.answer(&job(&paths[0], 64)).is_none());
        assert!(cache.answer(&job(&paths[1], 64)).is_some());
        assert!(cache.answer(&job(&paths[CACHE_CAPACITY], 64)).is_some());
    }
}
