use std::{collections::VecDeque, path::PathBuf};

use fast_image_resize::{
    PixelType,
    ResizeOptions,
    Resizer,
    images::{Image, ImageRef},
};
use image::{DynamicImage, RgbaImage};
use kernel::update::{Machine, Rejected};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Decoding {
    #[default]
    Idle,
    Busy(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverRequest {
    pub path: PathBuf,
    pub side: u32,
}

impl From<&CoverRequest> for &'static str {
    fn from(_: &CoverRequest) -> Self {
        "cover"
    }
}

#[derive(Debug)]
pub enum CoverOutcome {
    Art(RgbaImage),
    NoArt,
    Failed(CoverError),
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
    pub side: u32,
    pub outcome: CoverOutcome,
}

#[derive(Debug)]
pub(crate) enum DecodeMessage {
    Request(CoverRequest),
    Decoded(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodingRejection {
    WhileBusy,
    WhileIdle,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum DecodeIo {
    Decode(CoverRequest),
    #[default]
    Nothing,
}

type Step = Result<(Decoding, DecodeIo), Rejected<Decoding>>;

impl Machine for Decoding {
    type Message = DecodeMessage;
    type Rejection = DecodingRejection;
    type Effect = DecodeIo;

    fn transition(self, message: DecodeMessage) -> Step {
        match (self, message) {
            (Decoding::Idle, DecodeMessage::Request(request)) => Ok(requested(request)),
            (state @ Decoding::Idle, DecodeMessage::Decoded(_)) => Err(Rejected {
                state,
                reason: DecodingRejection::WhileIdle,
            }),
            (Decoding::Busy(path), DecodeMessage::Request(request))
                if path != request.path =>
            {
                Ok(requested(request))
            }
            (Decoding::Busy(path), DecodeMessage::Decoded(answered))
                if path == answered =>
            {
                Ok((Decoding::Idle, DecodeIo::Nothing))
            }
            (
                state @ Decoding::Busy(_),
                DecodeMessage::Request(_) | DecodeMessage::Decoded(_),
            ) => Err(Rejected {
                state,
                reason: DecodingRejection::WhileBusy,
            }),
        }
    }
}

fn requested(request: CoverRequest) -> (Decoding, DecodeIo) {
    let path = request.path.clone();
    (Decoding::Busy(path), DecodeIo::Decode(request))
}

pub(crate) const CACHE_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CachedOutcome {
    Art(RgbaImage),
    NoArt,
}

impl CachedOutcome {
    pub(crate) fn from_outcome(outcome: &CoverOutcome) -> Option<Self> {
        match outcome {
            CoverOutcome::Art(image) => Some(CachedOutcome::Art(image.clone())),
            CoverOutcome::NoArt => Some(CachedOutcome::NoArt),
            CoverOutcome::Failed(_) => None,
        }
    }

    fn into_outcome(self) -> CoverOutcome {
        match self {
            CachedOutcome::Art(image) => CoverOutcome::Art(image),
            CachedOutcome::NoArt => CoverOutcome::NoArt,
        }
    }
}

#[derive(Debug, PartialEq)]
struct CoverCacheEntry {
    path: PathBuf,
    side: u32,
    outcome: CachedOutcome,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct CoverCache {
    entries: VecDeque<CoverCacheEntry>,
}

impl CoverCache {
    pub(crate) fn answer(&mut self, request: &CoverRequest) -> Option<CoverOutcome> {
        let position = self.entries.iter().position(|entry| {
            entry.path == request.path && entry.side == request.side
        })?;
        let entry = self.entries.remove(position)?;
        let outcome = entry.outcome.clone();
        self.entries.push_front(entry);
        Some(outcome.into_outcome())
    }

    pub(crate) fn remember(&mut self, done: &CoverDone) {
        let Some(cached) = &done.cached else {
            return;
        };
        self.entries
            .retain(|entry| entry.path != done.path || entry.side != done.side);
        if self.entries.len() == CACHE_CAPACITY {
            self.entries.pop_back();
        }
        self.entries.push_front(CoverCacheEntry {
            path: done.path.clone(),
            side: done.side,
            outcome: cached.clone(),
        });
    }
}

#[derive(Debug)]
pub(crate) struct CoverDone {
    pub(crate) path: PathBuf,
    pub(crate) side: u32,
    pub(crate) cached: Option<CachedOutcome>,
}

pub(crate) fn decode(request: &CoverRequest) -> CoverDecoded {
    let outcome = library::embedded_cover(&request.path)
        .map_or(CoverOutcome::NoArt, |bytes| {
            decode_bytes(&bytes, request.side)
        });
    CoverDecoded {
        path: request.path.clone(),
        side: request.side,
        outcome,
    }
}

fn decode_bytes(bytes: &[u8], side: u32) -> CoverOutcome {
    match image::load_from_memory(bytes) {
        Ok(decoded) => {
            fit_square(decoded, side).map_or(CoverOutcome::NoArt, CoverOutcome::Art)
        }
        Err(error) => CoverOutcome::Failed(CoverError { source: error }),
    }
}

pub(crate) fn fit_square(image: DynamicImage, side: u32) -> Option<RgbaImage> {
    let source = image.into_rgba8();
    let (source_width, source_height) = source.dimensions();
    if source_width == 0 || source_height == 0 {
        return None;
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
    .ok()?;
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
        .ok()?;
    RgbaImage::from_raw(side, side, target.into_vec())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf, time::Duration};

    use crossbeam_channel::Receiver;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use kernel::{Message, update::Machine};
    use library::LibraryPaths;
    use rstest::rstest;

    use crate::{
        cells::{Cells, cells},
        driver::DriverThread,
        interpret::LibraryCommand,
        library::{
            cover::{
                CACHE_CAPACITY,
                CoverOutcome,
                CoverRequest,
                DecodeIo,
                DecodeMessage,
                Decoding,
                DecodingRejection,
                decode,
                fit_square,
            },
            driver::spawn,
        },
    };

    fn request(path: &str, side: u32) -> CoverRequest {
        CoverRequest {
            path: PathBuf::from(path),
            side,
        }
    }

    fn idle() -> Decoding {
        Decoding::Idle
    }

    fn busy(path: &str) -> Decoding {
        Decoding::Busy(PathBuf::from(path))
    }

    fn render(io: &DecodeIo) -> String {
        match io {
            DecodeIo::Decode(request) => {
                format!("decode {} @ {}", request.path.display(), request.side)
            }
            DecodeIo::Nothing => "nothing".to_string(),
        }
    }

    struct Cell {
        start: Decoding,
        message: DecodeMessage,
        next: Decoding,
        io: &'static str,
    }

    #[rstest]
    #[case::idle_starts_a_decode(Cell {
        start: idle(),
        message: DecodeMessage::Request(request("/music/cover.jpg", 64)),
        next: busy("/music/cover.jpg"),
        io: "decode /music/cover.jpg @ 64",
    })]
    #[case::busy_switches_to_another_path(Cell {
        start: busy("/music/one.jpg"),
        message: DecodeMessage::Request(request("/music/two.jpg", 64)),
        next: busy("/music/two.jpg"),
        io: "decode /music/two.jpg @ 64",
    })]
    #[case::busy_settles_on_its_own_answer(Cell {
        start: busy("/music/cover.jpg"),
        message: DecodeMessage::Decoded(PathBuf::from("/music/cover.jpg")),
        next: idle(),
        io: "nothing",
    })]
    fn a_cell_moves_the_decode_and_names_its_io(#[case] cell: Cell) {
        let (state, effect) = cell.start.transition(cell.message).unwrap();
        assert_eq!(state, cell.next);
        assert_eq!(render(&effect), cell.io);
    }

    #[rstest]
    #[case::idle_refuses_an_answer(
        idle(),
        DecodeMessage::Decoded(PathBuf::from("/music/cover.jpg")),
        DecodingRejection::WhileIdle
    )]
    #[case::busy_refuses_the_same_request_again(
        busy("/music/cover.jpg"),
        DecodeMessage::Request(request("/music/cover.jpg", 64)),
        DecodingRejection::WhileBusy
    )]
    #[case::busy_refuses_an_unrelated_answer(
        busy("/music/cover.jpg"),
        DecodeMessage::Decoded(PathBuf::from("/music/other.jpg")),
        DecodingRejection::WhileBusy
    )]
    fn a_refused_cell_hands_the_state_back(
        #[case] start: Decoding,
        #[case] message: DecodeMessage,
        #[case] reason: DecodingRejection,
    ) {
        let expected = start.clone();
        let rejected = start.transition(message).err().unwrap();
        assert_eq!(rejected.state, expected);
        assert_eq!(rejected.reason, reason);
    }

    #[test]
    fn fit_square_center_crops_and_resizes_to_the_requested_side() {
        let wide = DynamicImage::ImageRgba8(RgbaImage::new(300, 200));
        let side = 48;

        let fitted = fit_square(wide, side);

        assert_eq!(
            fitted.map(|image| (image.width(), image.height())),
            Some((side, side))
        );
    }

    #[test]
    fn fit_square_treats_an_empty_decode_as_no_art() {
        let empty = DynamicImage::ImageRgba8(RgbaImage::new(0, 0));

        assert!(fit_square(empty, 48).is_none());
    }

    #[test]
    fn a_request_for_a_file_without_a_tag_decodes_as_no_art() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("untagged.mp3");
        std::fs::write(&path, b"not an audio file").unwrap();

        let decoded = decode(&request(path.to_str().unwrap(), 64));

        assert!(matches!(decoded.outcome, CoverOutcome::NoArt));
    }

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const DECODABLE: &[&str] = &["mp3"];

    fn paths(directory: &tempfile::TempDir) -> LibraryPaths {
        LibraryPaths {
            cache: directory.path().join("cache"),
            data: directory.path().join("data"),
            playlists: directory.path().join("playlists"),
        }
    }

    fn spawned_with_covers(
        directory: &tempfile::TempDir,
    ) -> (
        DriverThread<LibraryCommand>,
        Cells,
        Receiver<()>,
        Receiver<Message>,
    ) {
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (writers, cells, doorbell) = cells();
        let thread =
            spawn((paths(directory), DECODABLE), &mailbox, writers.cover).unwrap();
        (thread, cells, doorbell, messages)
    }

    fn send_cover(thread: &DriverThread<LibraryCommand>, request: CoverRequest) {
        thread
            .commands
            .send(LibraryCommand::Cover(request))
            .unwrap();
    }

    fn recv_cover(
        cells: &Cells,
        doorbell: &Receiver<()>,
    ) -> crate::library::cover::CoverDecoded {
        doorbell.recv_timeout(RECV_TIMEOUT).unwrap();
        let decoded = cells.cover.take().unwrap();
        std::sync::Arc::try_unwrap(decoded).unwrap()
    }

    fn stopped(thread: DriverThread<LibraryCommand>) {
        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn png_bytes(fill: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(4, 4, Rgba(fill));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn minimal_flac_with_picture(picture_data: &[u8]) -> Vec<u8> {
        let mime = b"image/png";
        let description: &[u8] = b"";

        let mut picture_payload = Vec::new();
        picture_payload.extend_from_slice(&3u32.to_be_bytes());
        picture_payload
            .extend_from_slice(&u32::try_from(mime.len()).unwrap_or(0).to_be_bytes());
        picture_payload.extend_from_slice(mime);
        picture_payload.extend_from_slice(
            &u32::try_from(description.len()).unwrap_or(0).to_be_bytes(),
        );
        picture_payload.extend_from_slice(description);
        picture_payload.extend_from_slice(&4u32.to_be_bytes());
        picture_payload.extend_from_slice(&4u32.to_be_bytes());
        picture_payload.extend_from_slice(&32u32.to_be_bytes());
        picture_payload.extend_from_slice(&0u32.to_be_bytes());
        picture_payload.extend_from_slice(
            &u32::try_from(picture_data.len()).unwrap_or(0).to_be_bytes(),
        );
        picture_payload.extend_from_slice(picture_data);

        let streaminfo_bits: u64 = (44_100u64 << 44) | (1u64 << 41) | (15u64 << 36);
        let mut streaminfo = Vec::new();
        streaminfo.extend_from_slice(&4096u16.to_be_bytes());
        streaminfo.extend_from_slice(&4096u16.to_be_bytes());
        streaminfo.extend_from_slice(&[0, 0, 0]);
        streaminfo.extend_from_slice(&[0, 0, 0]);
        streaminfo.extend_from_slice(&streaminfo_bits.to_be_bytes());
        streaminfo.extend_from_slice(&[0u8; 16]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"fLaC");
        bytes.push(0x00);
        bytes.extend_from_slice(&[0x00, 0x00, 0x22]);
        bytes.extend_from_slice(&streaminfo);
        bytes.push(0x86);
        let payload_length = picture_payload.len().to_be_bytes();
        bytes.extend_from_slice(&payload_length[payload_length.len() - 3..]);
        bytes.extend_from_slice(&picture_payload);
        bytes
    }

    fn flac_with_cover(
        directory: &tempfile::TempDir,
        name: &str,
        fill: [u8; 4],
    ) -> PathBuf {
        let path = directory.path().join(name);
        std::fs::write(&path, minimal_flac_with_picture(&png_bytes(fill))).unwrap();
        path
    }

    #[test]
    fn a_repeated_cover_request_answers_from_the_cache_once_the_file_is_gone() {
        let directory = tempfile::tempdir().unwrap();
        let path = flac_with_cover(&directory, "cover.flac", [10, 20, 30, 255]);
        let (thread, cells, doorbell, _messages) = spawned_with_covers(&directory);

        send_cover(
            &thread,
            CoverRequest {
                path: path.clone(),
                side: 8,
            },
        );
        let first = recv_cover(&cells, &doorbell);
        assert!(matches!(first.outcome, CoverOutcome::Art(_)));

        std::fs::remove_file(&path).unwrap();
        send_cover(&thread, CoverRequest { path, side: 8 });
        let second = recv_cover(&cells, &doorbell);
        assert!(
            matches!(second.outcome, CoverOutcome::Art(_)),
            "expected a cached Art answer, got {:?}",
            second.outcome
        );

        stopped(thread);
    }

    #[test]
    fn the_ninth_distinct_cover_evicts_the_oldest_cached_entry() {
        let directory = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..=CACHE_CAPACITY)
            .map(|index| {
                let fill = [u8::try_from(index).unwrap_or(255), 0, 0, 255];
                flac_with_cover(&directory, &format!("cover{index}.flac"), fill)
            })
            .collect();
        let (thread, cells, doorbell, _messages) = spawned_with_covers(&directory);

        for path in &paths {
            send_cover(
                &thread,
                CoverRequest {
                    path: path.clone(),
                    side: 8,
                },
            );
            let answer = recv_cover(&cells, &doorbell);
            assert!(matches!(answer.outcome, CoverOutcome::Art(_)));
        }

        let oldest = paths.first().unwrap();
        std::fs::remove_file(oldest).unwrap();
        send_cover(
            &thread,
            CoverRequest {
                path: oldest.clone(),
                side: 8,
            },
        );
        let evicted_answer = recv_cover(&cells, &doorbell);
        assert!(
            matches!(evicted_answer.outcome, CoverOutcome::NoArt),
            "expected the evicted entry to force a fresh decode and find the file gone, \
             got {:?}",
            evicted_answer.outcome
        );

        stopped(thread);
    }

    #[test]
    fn a_failed_decode_is_not_cached() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.flac");
        std::fs::write(&path, minimal_flac_with_picture(b"not a real image")).unwrap();
        let (thread, cells, doorbell, _messages) = spawned_with_covers(&directory);

        send_cover(
            &thread,
            CoverRequest {
                path: path.clone(),
                side: 8,
            },
        );
        let first = recv_cover(&cells, &doorbell);
        assert!(matches!(first.outcome, CoverOutcome::Failed(_)));

        std::fs::remove_file(&path).unwrap();
        send_cover(&thread, CoverRequest { path, side: 8 });
        let second = recv_cover(&cells, &doorbell);
        assert!(
            matches!(second.outcome, CoverOutcome::NoArt),
            "expected an uncached failure to force a fresh decode and find the file gone, \
             got {:?}",
            second.outcome
        );

        stopped(thread);
    }
}
