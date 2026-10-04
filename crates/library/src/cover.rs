use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use fast_image_resize::{
    PixelType,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::{DynamicImage, RgbaImage};
use kernel::{
    cmd::{Cmd, CoverJob},
    domain::{geometry::Pixels, revision::Revision},
    update::machine::{Machine, Unhandled},
};

use crate::{driver::LibraryMessage, tags::embedded_cover};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum CoverDecoding {
    #[default]
    Idle,
    Busy {
        job: CoverJob,
        revision: Revision,
    },
}

#[derive(Debug)]
pub enum CoverArt {
    Image(Arc<RgbaImage>),
    Missing,
    Error(CoverError),
}

#[derive(Debug, thiserror::Error)]
#[error("cannot decode embedded art: {source}")]
pub struct CoverError {
    #[source]
    pub source: image::ImageError,
}

#[derive(Debug)]
pub struct CoverDecoded {
    pub path: PathBuf,
    pub side: Pixels,
    pub art: CoverArt,
}

#[derive(Debug)]
pub(crate) enum CoverDecodingMessage {
    Request { job: CoverJob, revision: Revision },
    Decoded(Revision),
}

impl Machine for CoverDecoding {
    type Message = CoverDecodingMessage;
    type Effect = Cmd<(CoverJob, Revision), LibraryMessage>;

    fn transition(
        &mut self,
        message: CoverDecodingMessage,
    ) -> Result<Cmd<(CoverJob, Revision), LibraryMessage>, Unhandled> {
        match (&*self, message) {
            (CoverDecoding::Idle, CoverDecodingMessage::Request { job, revision }) => {
                Ok(self.start(job, revision))
            }
            (
                CoverDecoding::Busy { job: busy, .. },
                CoverDecodingMessage::Request { job, revision },
            ) if busy.path != job.path => Ok(self.start(job, revision)),
            (
                CoverDecoding::Busy { revision, .. },
                CoverDecodingMessage::Decoded(answered),
            ) if *revision == answered => {
                *self = CoverDecoding::Idle;
                Ok(Cmd::none())
            }
            (CoverDecoding::Idle, CoverDecodingMessage::Decoded(_))
            | (
                CoverDecoding::Busy { .. },
                CoverDecodingMessage::Request { .. } | CoverDecodingMessage::Decoded(_),
            ) => Err(Unhandled),
        }
    }
}

impl CoverDecoding {
    fn start(
        &mut self,
        job: CoverJob,
        revision: Revision,
    ) -> Cmd<(CoverJob, Revision), LibraryMessage> {
        *self = CoverDecoding::Busy {
            job: job.clone(),
            revision,
        };
        Cmd::effect((job, revision))
    }
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
            CoverArt::Error(_) => return,
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

pub(crate) fn decode(job: CoverJob) -> CoverDecoded {
    let art = embedded_cover(&job.path)
        .map_or(CoverArt::Missing, |bytes| decode_bytes(&bytes, job.side));
    CoverDecoded {
        path: job.path,
        side: job.side,
        art,
    }
}

fn decode_bytes(bytes: &[u8], side: Pixels) -> CoverArt {
    match image::load_from_memory(bytes) {
        Ok(decoded) => match fit_square(decoded, side.0) {
            Ok(Some(image)) => CoverArt::Image(Arc::new(image)),
            Ok(None) => CoverArt::Missing,
            Err(error) => CoverArt::Error(error),
        },
        Err(error) => CoverArt::Error(CoverError { source: error }),
    }
}

fn resize_failed(error: &impl std::fmt::Display) -> CoverError {
    CoverError {
        source: image::ImageError::Parameter(image::error::ParameterError::from_kind(
            image::error::ParameterErrorKind::Generic(error.to_string()),
        )),
    }
}

pub(crate) fn fit_square(
    image: DynamicImage,
    side: u32,
) -> Result<Option<RgbaImage>, CoverError> {
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
    use kernel::{
        cmd::{Cmd, CoverJob},
        domain::{geometry::Pixels, revision::Revision},
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{
            CACHE_CAPACITY,
            CoverArt,
            CoverCache,
            CoverDecoded,
            CoverDecoding,
            CoverDecodingMessage,
            CoverError,
            decode,
            fit_square,
        },
        driver::LibraryMessage,
    };

    fn job(path: &str, side: u32) -> CoverJob {
        CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        }
    }

    fn request(path: &str) -> CoverDecodingMessage {
        CoverDecodingMessage::Request {
            job: job(path, 64),
            revision: Revision::default(),
        }
    }

    fn idle() -> CoverDecoding {
        CoverDecoding::Idle
    }

    fn busy(path: &str) -> CoverDecoding {
        CoverDecoding::Busy {
            job: job(path, 64),
            revision: Revision::default(),
        }
    }

    fn describe(cmd: &Cmd<(CoverJob, Revision), LibraryMessage>) -> String {
        cmd.effects().next().map_or_else(
            || "nothing".to_string(),
            |(job, _)| format!("decode {} @ {}", job.path.display(), job.side.0),
        )
    }

    struct CoverDecodingRow {
        start: CoverDecoding,
        message: CoverDecodingMessage,
        next: CoverDecoding,
        effect: &'static str,
    }

    #[rstest]
    #[case::idle_starts_a_decode(CoverDecodingRow {
        start: idle(),
        message: request("/music/cover.jpg"),
        next: busy("/music/cover.jpg"),
        effect: "decode /music/cover.jpg @ 64",
    })]
    #[case::busy_switches_to_another_path(CoverDecodingRow {
        start: busy("/music/one.jpg"),
        message: request("/music/two.jpg"),
        next: busy("/music/two.jpg"),
        effect: "decode /music/two.jpg @ 64",
    })]
    #[case::busy_settles_on_its_own_answer(CoverDecodingRow {
        start: busy("/music/cover.jpg"),
        message: CoverDecodingMessage::Decoded(Revision::default()),
        next: idle(),
        effect: "nothing",
    })]
    fn a_row_moves_the_decode_and_names_its_effect(#[case] row: CoverDecodingRow) {
        let mut state = row.start;
        let effect = state.transition(row.message).unwrap();
        assert_eq!(state, row.next);
        assert_eq!(describe(&effect), row.effect);
    }

    #[rstest]
    #[case::idle_refuses_an_answer(
        idle(),
        CoverDecodingMessage::Decoded(Revision::default())
    )]
    #[case::busy_refuses_the_same_request_again(
        busy("/music/cover.jpg"),
        request("/music/cover.jpg")
    )]
    #[case::busy_refuses_a_stale_answer(
        busy("/music/cover.jpg"),
        CoverDecodingMessage::Decoded(Revision::default().next())
    )]
    fn a_refused_row_hands_the_state_back(
        #[case] start: CoverDecoding,
        #[case] message: CoverDecodingMessage,
    ) {
        let expected = start.clone();
        let mut state = start;
        let refused = state.transition(message).err().unwrap();
        assert_eq!(state, expected);
        assert_eq!(refused, Unhandled);
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
        let path = directory.path().join("untagged.mp3");
        std::fs::write(&path, b"not an audio file").unwrap();

        let decoded = decode(job(path.to_str().unwrap(), 64));

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
    fn a_failed_decode_is_not_cached() {
        let mut cache = CoverCache::default();
        let error = image::load_from_memory(b"not an image")
            .expect_err("garbage must fail to decode");
        cache.remember(&decoded(
            "/music/one.flac",
            CoverArt::Error(CoverError { source: error }),
        ));

        assert!(cache.answer(&job("/music/one.flac", 64)).is_none());
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
