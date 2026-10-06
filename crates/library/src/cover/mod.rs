use std::{
    collections::VecDeque,
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct CoverDecoded {
    pub path: PathBuf,
    pub side: Pixels,
    pub art: CoverArt,
}

pub(crate) const CACHE_CAPACITY: usize = 8;

#[derive(Debug, Default)]
pub(crate) struct CoverCache {
    decodeds: VecDeque<CoverDecoded>,
}

impl CoverCache {
    pub(crate) fn answer(&self, job: &CoverJob) -> Option<&CoverDecoded> {
        self.decodeds
            .iter()
            .find(|entry| entry.path == job.path && entry.side == job.side)
    }

    pub(crate) fn remember(&mut self, decoded: &CoverDecoded) {
        self.decodeds
            .retain(|entry| entry.path != decoded.path || entry.side != decoded.side);
        if self.decodeds.len() == CACHE_CAPACITY {
            self.decodeds.pop_back();
        }
        self.decodeds.push_front(decoded.clone());
    }
}

const COVER_NAMES: [&str; 6] = [
    "cover.jpg",
    "cover.png",
    "folder.jpg",
    "folder.png",
    "front.jpg",
    "front.png",
];

pub fn cover_bytes(path: &Path) -> io::Result<Vec<u8>> {
    if let Ok(Some(bytes)) = embedded_cover(path) {
        return Ok(bytes);
    }
    folder_cover(path)
        .map_or_else(|| Err(io::Error::from(io::ErrorKind::NotFound)), fs::read)
}

fn folder_cover(path: &Path) -> Option<PathBuf> {
    let folder = path.parent()?;
    COVER_NAMES
        .iter()
        .map(|name| folder.join(name))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn decode(job: CoverJob) -> Result<CoverDecoded, CoverError> {
    let art = match cover_bytes(&job.path) {
        Ok(bytes) => decode_bytes(&bytes, job.side),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(CoverArt::Missing),
        Err(error) => Err(image::ImageError::IoError(error)),
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
    use std::{
        fs,
        io,
        io::Cursor,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::Arc,
    };

    use image::{DynamicImage, ImageFormat, RgbaImage};
    use kernel::{cmd::CoverJob, domain::geometry::Pixels};

    use crate::{
        cover::{
            CACHE_CAPACITY,
            CoverArt,
            CoverCache,
            CoverDecoded,
            cover_bytes,
            decode,
            fit_square,
        },
        test_support::minimal_flac_with_cover,
    };

    fn untagged_track(directory: &tempfile::TempDir) -> PathBuf {
        let path = directory.path().join("untagged.wav");
        fs::write(&path, include_bytes!("../../tests/fixtures/tone.wav")).unwrap();
        path
    }

    #[test]
    fn the_embedded_cover_wins_over_the_folder_cover() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("cover.jpg"), b"jpg").unwrap();
        let path = directory.path().join("track.flac");
        fs::write(&path, minimal_flac_with_cover(b"embedded")).unwrap();

        assert_eq!(cover_bytes(&path).unwrap(), b"embedded".to_vec());
    }

    #[test]
    fn the_folder_cover_is_used_when_the_track_has_no_embedded_art() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("folder.png"), b"png").unwrap();
        let path = untagged_track(&directory);

        assert_eq!(cover_bytes(&path).unwrap(), b"png".to_vec());
    }

    fn unparseable_track(directory: &tempfile::TempDir) -> PathBuf {
        let path = directory.path().join("clip.mkv");
        fs::write(&path, b"not a real container").unwrap();
        path
    }

    #[test]
    fn the_folder_cover_is_used_when_the_embedded_cover_cannot_be_parsed() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("cover.jpg"), b"jpg").unwrap();
        let path = unparseable_track(&directory);

        assert_eq!(cover_bytes(&path).unwrap(), b"jpg".to_vec());
    }

    #[test]
    fn an_unparseable_track_without_a_folder_cover_decodes_as_missing_art() {
        let directory = tempfile::tempdir().unwrap();
        let path = unparseable_track(&directory);

        let decoded = decode(job(path.to_str().unwrap(), 64)).unwrap();

        assert!(matches!(decoded.art, CoverArt::Missing));
    }

    #[test]
    fn a_track_with_neither_cover_is_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let path = untagged_track(&directory);

        let error = cover_bytes(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn an_unreadable_folder_cover_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let cover = directory.path().join("cover.jpg");
        fs::write(&cover, b"jpg").unwrap();
        fs::set_permissions(&cover, fs::Permissions::from_mode(0o000)).unwrap();
        let path = untagged_track(&directory);

        let error = cover_bytes(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_job_for_an_untagged_track_decodes_the_folder_cover() {
        let directory = tempfile::tempdir().unwrap();
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::new(8, 8))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        fs::write(directory.path().join("cover.png"), png).unwrap();
        let path = untagged_track(&directory);

        let decoded = decode(job(path.to_str().unwrap(), 4)).unwrap();

        assert!(matches!(decoded.art, CoverArt::Image(_)));
    }

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
        fs::write(&path, include_bytes!("../../tests/fixtures/tone.wav")).unwrap();

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
