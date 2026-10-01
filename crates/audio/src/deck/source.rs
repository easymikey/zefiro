use std::{
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

use crossbeam_channel::Sender;
use kernel::{AudioError, domain::Speed};
use rodio::Source;

use crate::{
    deck::{
        DeckEvent,
        Ticket,
        envelope::{Envelopes, envelope},
        output::Output,
        worker::{AudioWorker, DecodeRequest, Job},
    },
    error::Error,
};

pub(crate) type TrackDecoder = rodio::Decoder<BufReader<File>>;
pub(crate) type DecodeResult = Result<TrackDecoder, Error>;

const READ_CAPACITY: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreloadRequest {
    Gapless(PathBuf),
    Crossfade {
        path: PathBuf,
        gain: Option<f32>,
        speed: Speed,
    },
}

impl PreloadRequest {
    pub(crate) fn path(&self) -> &Path {
        match self {
            PreloadRequest::Gapless(path) | PreloadRequest::Crossfade { path, .. } => {
                path
            }
        }
    }
}

pub(crate) fn decode(path: &Path) -> DecodeResult {
    let file = File::open(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file.metadata().map_or(0, |metadata| metadata.len());
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

pub(crate) struct Decoding {
    decode_ticket: Ticket,
    preload_ticket: Ticket,
    preloading: Option<PreloadRequest>,
    staged: Option<TrackDecoder>,
    worker: Result<AudioWorker, AudioError>,
    wake: Sender<DeckEvent>,
    pub(crate) envelopes: Envelopes,
}

impl Decoding {
    pub(crate) fn new(wake: Sender<DeckEvent>) -> Self {
        let worker =
            AudioWorker::spawn(wake.clone()).map_err(|error| AudioError::from(&error));
        Self {
            decode_ticket: Ticket::default(),
            preload_ticket: Ticket::default(),
            preloading: None,
            staged: None,
            worker,
            wake,
            envelopes: Envelopes::default(),
        }
    }

    pub(crate) fn start_decode(&mut self, path: PathBuf) -> Option<AudioError> {
        self.staged = None;
        self.decode_ticket = self.decode_ticket.next();
        match &self.worker {
            Ok(worker) => {
                worker.submit(Job::Decode(DecodeRequest {
                    path,
                    ticket: self.decode_ticket,
                }));
                None
            }
            Err(error) => Some(error.clone()),
        }
    }

    pub(crate) fn start_preload(
        &mut self,
        request: PreloadRequest,
    ) -> Option<AudioError> {
        self.preload_ticket = self.preload_ticket.next();
        let path = request.path().to_path_buf();
        let outcome = match &self.worker {
            Ok(worker) => {
                worker.submit(Job::Preload(DecodeRequest {
                    path,
                    ticket: self.preload_ticket,
                }));
                None
            }
            Err(error) => Some(error.clone()),
        };
        self.preloading = Some(request);
        outcome
    }

    pub(crate) fn list_devices(&self) {
        if let Ok(worker) = &self.worker {
            worker.submit(Job::ListDevices);
        }
    }

    pub(crate) fn drop_preload(&mut self) {
        self.preload_ticket = self.preload_ticket.next();
        self.preloading = None;
    }

    pub(crate) fn clear_staged(&mut self) {
        self.decode_ticket = self.decode_ticket.next();
        self.staged = None;
    }

    pub(crate) fn accept_decode(
        &mut self,
        ticket: Ticket,
        outcome: DecodeResult,
    ) -> Option<Result<Option<Duration>, AudioError>> {
        if ticket != self.decode_ticket {
            return None;
        }
        Some(match outcome {
            Ok(source) => {
                let total = source.total_duration();
                self.staged = Some(source);
                Ok(total)
            }
            Err(error) => Err(AudioError::from(&error)),
        })
    }

    pub(crate) fn take_preloading(&mut self, ticket: Ticket) -> Option<PreloadRequest> {
        if ticket != self.preload_ticket {
            return None;
        }
        self.preloading.take()
    }

    pub(crate) fn append_staged(&mut self, output: &Output) {
        let Some(source) = self.staged.take() else {
            return;
        };
        let (wrapped, control) =
            envelope(source, self.decode_ticket, self.wake.clone());
        output.append(wrapped);
        self.envelopes.primary = Some(control);
    }
}
