use std::{ffi::OsStr, fs::File, io::BufReader, path::Path, time::Duration};

use kernel::domain::{revision::Revision, speed::Speed};
use rodio::Source;

use crate::error::Error;

pub(crate) type TrackDecoder = rodio::Decoder<BufReader<File>>;

pub struct DecodedTrack {
    pub(crate) revision: Revision,
    pub(crate) decoder: TrackDecoder,
}

impl DecodedTrack {
    pub(crate) fn duration(&self) -> Option<Duration> {
        self.decoder.total_duration()
    }
}

impl PartialEq for DecodedTrack {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

impl std::fmt::Debug for DecodedTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedTrack")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

const READ_CAPACITY: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreloadMode {
    Gapless,
    Crossfade(Speed),
}

pub(crate) fn decode(path: &Path) -> Result<TrackDecoder, Error> {
    let opened = |source| Error::Open {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(opened)?;
    let length = file.metadata().map_err(opened)?.len();
    let reader = BufReader::with_capacity(READ_CAPACITY, file);
    let builder = rodio::Decoder::builder()
        .with_data(reader)
        .with_byte_len(length)
        .with_seekable(true);
    let builder = match path.extension().and_then(OsStr::to_str) {
        Some(hint) => builder.with_hint(hint),
        None => builder,
    };
    builder.build().map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })
}
