use std::{
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

use crossbeam_channel::Sender;
use kernel::domain::{Revision, Speed};
use rodio::Source;

use crate::{
    deck::{
        envelope::{Envelopes, envelope},
        output::Output,
    },
    engine::effect::AudioMessage,
    error::Error,
};

pub(crate) type TrackDecoder = rodio::Decoder<BufReader<File>>;

pub struct TrackSource {
    pub(crate) revision: Revision,
    pub(crate) source: TrackDecoder,
}

impl TrackSource {
    pub(crate) fn total(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

impl PartialEq for TrackSource {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

impl std::fmt::Debug for TrackSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackSource")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

const READ_CAPACITY: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum PreloadMode {
    Gapless,
    Crossfade {
        gain: Option<kernel::domain::Decibels>,
        speed: Speed,
    },
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

pub(crate) struct TrackDecoding {
    preloading: Option<(PathBuf, PreloadMode)>,
    staged: Option<TrackSource>,
    sender: Sender<AudioMessage>,
    pub(crate) envelopes: Envelopes,
}

impl TrackDecoding {
    pub(crate) fn new(sender: Sender<AudioMessage>) -> Self {
        Self {
            preloading: None,
            staged: None,
            sender,
            envelopes: Envelopes::default(),
        }
    }

    pub(crate) fn start_decode(&mut self) {
        self.staged = None;
    }

    pub(crate) fn start_preload(&mut self, path: PathBuf, mode: PreloadMode) {
        self.preloading = Some((path, mode));
    }

    pub(crate) fn drop_preload(&mut self) {
        self.preloading = None;
    }

    pub(crate) fn clear_staged(&mut self) {
        self.staged = None;
    }

    pub(crate) fn stage(&mut self, track: TrackSource) {
        self.staged = Some(track);
    }

    pub(crate) fn take_preloading(&mut self) -> Option<(PathBuf, PreloadMode)> {
        self.preloading.take()
    }

    pub(crate) fn append_staged(&mut self, output: &Output) {
        let Some(TrackSource { revision, source }) = self.staged.take() else {
            return;
        };
        let (wrapped, control) = envelope(source, revision, self.sender.clone());
        output.append(wrapped);
        self.envelopes.primary = Some(control);
    }
}
