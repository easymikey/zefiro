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
use image::{DynamicImage, RgbImage, RgbaImage};
use kernel::{cmd::CoverJob, domain::geometry::Pixels};

use crate::tags::embedded_cover;

pub(crate) mod decoding;

#[derive(Debug, Clone)]
pub enum CoverLookup {
    Found(Arc<RgbaImage>),
    Missing,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot decode embedded cover: {source}")]
pub struct CoverError {
    pub path: PathBuf,
    #[source]
    pub source: image::ImageError,
}

#[derive(Debug, Clone)]
pub struct CoverDecoded {
    pub path: PathBuf,
    pub side: Pixels,
    pub cover_lookup: CoverLookup,
}

pub(crate) const CACHE_CAPACITY: usize = 8;

#[derive(Debug, Default)]
pub(crate) struct CoverCache {
    decodeds: VecDeque<CoverDecoded>,
}

impl CoverCache {
    pub(crate) fn cached(&self, cover_job: &CoverJob) -> Option<&CoverDecoded> {
        self.decodeds
            .iter()
            .find(|entry| entry.path == cover_job.path && entry.side == cover_job.side)
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

pub(crate) fn decode(cover_job: CoverJob) -> Result<CoverDecoded, CoverError> {
    let cover_lookup = match cover_bytes(&cover_job.path) {
        Ok(bytes) => decode_bytes(&bytes, cover_job.side),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(CoverLookup::Missing)
        }
        Err(error) => Err(image::ImageError::IoError(error)),
    };
    match cover_lookup {
        Ok(cover_lookup) => Ok(CoverDecoded {
            path: cover_job.path,
            side: cover_job.side,
            cover_lookup,
        }),
        Err(source) => Err(CoverError {
            path: cover_job.path,
            source,
        }),
    }
}

fn decode_bytes(bytes: &[u8], side: Pixels) -> Result<CoverLookup, image::ImageError> {
    let fitted = fit_square(image::load_from_memory(bytes)?, side)?;
    Ok(fitted.map_or(CoverLookup::Missing, |image| {
        CoverLookup::Found(Arc::new(image))
    }))
}

fn resize_failed(error: &impl std::fmt::Display) -> image::ImageError {
    image::ImageError::Parameter(image::error::ParameterError::from_kind(
        image::error::ParameterErrorKind::Generic(error.to_string()),
    ))
}

pub(crate) fn fit_square(
    image: DynamicImage,
    side: Pixels,
) -> Result<Option<RgbaImage>, image::ImageError> {
    let (source_width, source_height) = (image.width(), image.height());
    if source_width == 0 || source_height == 0 {
        return Ok(None);
    }
    let side = side.0.max(1);
    let crop = source_width.min(source_height);
    let crop_x = (source_width - crop) / 2;
    let crop_y = (source_height - crop) / 2;
    let options = ResizeOptions::new()
        .crop(
            f64::from(crop_x),
            f64::from(crop_y),
            f64::from(crop),
            f64::from(crop),
        )
        .use_alpha(false);
    let resize = |buffer: &[u8], pixel_type: PixelType| {
        let source_view =
            ImageRef::new(source_width, source_height, buffer, pixel_type)
                .map_err(|error| resize_failed(&error))?;
        let mut target = Image::new(side, side, pixel_type);
        Resizer::new()
            .resize(&source_view, &mut target, &options)
            .map_err(|error| resize_failed(&error))?;
        Ok::<_, image::ImageError>(target.into_vec())
    };
    if let Some(source) = image.as_rgb8() {
        let fitted = resize(source.as_raw(), PixelType::U8x3)?;
        return Ok(RgbImage::from_raw(side, side, fitted)
            .map(|rgb| DynamicImage::ImageRgb8(rgb).into_rgba8()));
    }
    let fitted = resize(image.into_rgba8().as_raw(), PixelType::U8x4)?;
    Ok(RgbaImage::from_raw(side, side, fitted))
}

#[cfg(test)]
mod tests {
    use std::{fs, io, io::Cursor, os::unix::fs::PermissionsExt, path::PathBuf};

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage, RgbaImage};
    use kernel::{cmd::CoverJob, domain::geometry::Pixels};
    use rstest::rstest;

    use crate::{
        cover::{CoverLookup, cover_bytes, decode, fit_square},
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
    fn the_folder_cover_is_used_when_the_track_has_no_embedded_cover() {
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
    fn an_unparseable_track_without_a_folder_cover_decodes_as_missing_cover() {
        let directory = tempfile::tempdir().unwrap();
        let path = unparseable_track(&directory);

        let decoded = decode(cover_job(path.to_str().unwrap(), 64)).unwrap();

        assert!(matches!(decoded.cover_lookup, CoverLookup::Missing));
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

        let decoded = decode(cover_job(path.to_str().unwrap(), 4)).unwrap();

        assert!(matches!(decoded.cover_lookup, CoverLookup::Found(_)));
    }

    fn cover_job(path: &str, side: u32) -> CoverJob {
        CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        }
    }

    #[rstest]
    #[case::a_transparent_rgba_cover(
        DynamicImage::ImageRgba8(RgbaImage::new(300, 200)),
        0
    )]
    #[case::an_rgb_cover_turns_opaque(
        DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 200, Rgb([10, 20, 30]))),
        u8::MAX
    )]
    fn fit_square_center_crops_and_resizes_to_the_requested_side(
        #[case] wide: DynamicImage,
        #[case] alpha: u8,
    ) {
        let side = Pixels(48);

        let fitted = fit_square(wide, side).unwrap().unwrap();

        assert_eq!(fitted.dimensions(), (side.0, side.0));
        assert!(fitted.pixels().all(|pixel| pixel.0[3] == alpha));
    }

    fn bands(across: u32, along: u32, band_of: fn(u32, u32) -> u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(across, along, |x, y| match band_of(
            x, y,
        ) {
            0 => Rgb([255, 0, 0]),
            1 => Rgb([0, 255, 0]),
            _ => Rgb([0, 0, 255]),
        }))
    }

    #[rstest]
    #[case::a_wide_cover_keeps_its_middle_columns(bands(24, 8, |x, _| x / 8))]
    #[case::a_tall_cover_keeps_its_middle_rows(bands(8, 24, |_, y| y / 8))]
    fn fit_square_keeps_the_middle_of_the_long_side(#[case] image: DynamicImage) {
        let fitted = fit_square(image, Pixels(8)).unwrap().unwrap();

        assert!(fitted.pixels().all(|pixel| pixel.0 == [0, 255, 0, u8::MAX]));
    }

    #[rstest]
    #[case::no_pixels(DynamicImage::ImageRgba8(RgbaImage::new(0, 0)))]
    #[case::no_columns(DynamicImage::ImageRgb8(RgbImage::new(0, 4)))]
    #[case::no_rows(DynamicImage::ImageRgb8(RgbImage::new(4, 0)))]
    fn fit_square_treats_an_empty_decode_as_missing_cover(#[case] empty: DynamicImage) {
        assert!(matches!(fit_square(empty, Pixels(48)), Ok(None)));
    }
}
