use fast_image_resize::{
    CropBox,
    ImageBufferError,
    PixelType,
    ResizeError,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::RgbaImage;
use thiserror::Error;

use crate::pixels::numeric::{dimension_f32, round};

pub(crate) fn cover_crop_resize(
    image: &RgbaImage,
    target_width: u32,
    target_height: u32,
) -> RgbaImage {
    let (source_width, source_height) = image.dimensions();
    let (target_width, target_height) = (target_width.max(1), target_height.max(1));
    if (source_width, source_height) == (target_width, target_height) {
        return image.clone();
    }
    if source_width == 0 || source_height == 0 {
        return RgbaImage::new(target_width, target_height);
    }

    let target_aspect = dimension_f32(target_width) / dimension_f32(target_height);
    let source_aspect = dimension_f32(source_width) / dimension_f32(source_height);
    let (crop_width, crop_height) = if source_aspect > target_aspect {
        let crop_height = source_height;
        let crop_width = round::<u32>(dimension_f32(source_height) * target_aspect)
            .clamp(1, source_width);
        (crop_width, crop_height)
    } else {
        let crop_width = source_width;
        let crop_height = round::<u32>(dimension_f32(source_width) / target_aspect)
            .clamp(1, source_height);
        (crop_width, crop_height)
    };
    let crop = CropBox {
        left: f64::from((source_width - crop_width) / 2),
        top: f64::from((source_height - crop_height) / 2),
        width: f64::from(crop_width),
        height: f64::from(crop_height),
    };
    match resample(image, crop, (target_width, target_height)) {
        Ok(resized) => resized,
        Err(
            ResampleError::SourceBuffer(_)
            | ResampleError::Resize(_)
            | ResampleError::TargetBuffer { .. },
        ) => RgbaImage::new(target_width, target_height),
    }
}

#[derive(Debug, Error)]
pub(crate) enum ResampleError {
    #[error("the source pixels do not fill their stated size")]
    SourceBuffer(#[source] ImageBufferError),
    #[error("the resizer refused the crop or the sizes")]
    Resize(#[source] ResizeError),
    #[error("the resized pixels do not fill the {width}x{height} target")]
    TargetBuffer { width: u32, height: u32 },
}

pub(crate) fn resample(
    image: &RgbaImage,
    crop: CropBox,
    target: (u32, u32),
) -> Result<RgbaImage, ResampleError> {
    let (source_width, source_height) = image.dimensions();
    let (target_width, target_height) = target;
    let source =
        ImageRef::new(source_width, source_height, image.as_raw(), PixelType::U8x4)
            .map_err(ResampleError::SourceBuffer)?;
    let mut resized = Image::new(target_width, target_height, PixelType::U8x4);
    let options = ResizeOptions::new()
        .crop(crop.left, crop.top, crop.width, crop.height)
        .use_alpha(false);
    Resizer::new()
        .resize(&source, &mut resized, &options)
        .map_err(ResampleError::Resize)?;
    RgbaImage::from_raw(target_width, target_height, resized.into_vec()).ok_or(
        ResampleError::TargetBuffer {
            width: target_width,
            height: target_height,
        },
    )
}

#[cfg(test)]
mod tests {
    use fast_image_resize::CropBox;
    use image::{Rgba, RgbaImage};
    use rstest::rstest;

    use crate::pixels::resample::{ResampleError, cover_crop_resize, resample};

    struct CropRow {
        source_width: u32,
        source_height: u32,
        target_width: u32,
        target_height: u32,
        crop: CropBox,
    }

    fn gradient(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                u8::try_from(x * 25).unwrap_or(u8::MAX),
                u8::try_from(y * 25).unwrap_or(u8::MAX),
                u8::try_from((x + y) * 12).unwrap_or(u8::MAX),
                255,
            ])
        })
    }

    #[rstest]
    #[case::a_wider_source_loses_its_side_columns(CropRow {
        source_width: 6,
        source_height: 4,
        target_width: 4,
        target_height: 4,
        crop: CropBox { left: 1.0, top: 0.0, width: 4.0, height: 4.0 },
    })]
    #[case::a_taller_source_loses_its_top_and_bottom_rows(CropRow {
        source_width: 4,
        source_height: 6,
        target_width: 4,
        target_height: 4,
        crop: CropBox { left: 0.0, top: 1.0, width: 4.0, height: 4.0 },
    })]
    #[case::a_wide_target_keeps_the_full_height_at_its_aspect(CropRow {
        source_width: 9,
        source_height: 2,
        target_width: 3,
        target_height: 1,
        crop: CropBox { left: 1.0, top: 0.0, width: 6.0, height: 2.0 },
    })]
    #[case::a_tall_target_keeps_the_full_width_at_its_aspect(CropRow {
        source_width: 2,
        source_height: 9,
        target_width: 1,
        target_height: 3,
        crop: CropBox { left: 0.0, top: 1.0, width: 2.0, height: 6.0 },
    })]
    fn cover_crop_resize_resamples_the_centred_crop_of_the_target_aspect(
        #[case] row: CropRow,
    ) {
        let CropRow {
            source_width,
            source_height,
            target_width,
            target_height,
            crop,
        } = row;
        let source = gradient(source_width, source_height);

        let resized = cover_crop_resize(&source, target_width, target_height);

        assert_eq!(
            Some(resized),
            resample(&source, crop, (target_width, target_height)).ok()
        );
    }

    #[test]
    fn resample_of_a_crop_beyond_the_source_answers_the_resize_error() {
        let source = RgbaImage::new(4, 4);
        let beyond = CropBox {
            left: 2.0,
            top: 0.0,
            width: 8.0,
            height: 4.0,
        };

        let outcome = resample(&source, beyond, (4, 4));

        assert!(matches!(outcome, Err(ResampleError::Resize(_))));
    }
}
