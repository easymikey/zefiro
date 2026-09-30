use std::{
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

use crossbeam_channel::Sender;
use kernel::AudioError;
use rodio::Source;

use crate::{
    deck::{
        DeckEvent,
        Ticket,
        envelope::{EnvelopeControl, Envelopes, envelope},
        output::Output,
        worker::{AudioWorker, DecodeRequest, Job},
    },
    engine::effect::{Preload, PreloadedTrack},
    error::{Error, preload_error},
};

pub(crate) type TrackDecoder = rodio::Decoder<BufReader<File>>;
pub(crate) type DecodeResult = Result<TrackDecoder, Error>;

const READ_CAPACITY: usize = 1 << 20;

pub(crate) enum PreloadRequest {
    Gapless(PathBuf),
    Crossfade {
        path: PathBuf,
        gain: Option<f32>,
        speed: f32,
    },
}

impl PreloadRequest {
    fn path(&self) -> &Path {
        match self {
            PreloadRequest::Gapless(path) | PreloadRequest::Crossfade { path, .. } => {
                path
            }
        }
    }
}

struct PendingPreload {
    request: PreloadRequest,
}

struct Decoded {
    source: TrackDecoder,
    total: Option<Duration>,
    ticket: Ticket,
}

struct InstallParts<'a> {
    output: Option<&'a mut Output>,
    wake: Sender<DeckEvent>,
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

fn install(
    request: PreloadRequest,
    decoded: Decoded,
    parts: InstallParts<'_>,
) -> (Option<Preload>, Option<EnvelopeControl>) {
    let Decoded {
        source,
        total,
        ticket,
    } = decoded;
    let InstallParts { output, wake } = parts;
    let Some(output) = output else {
        return (None, None);
    };
    let (wrapped, control) = envelope(source, ticket, wake);
    match request {
        PreloadRequest::Gapless(path) => {
            output.append(wrapped);
            (Some(Preload::Gapless(path)), Some(control))
        }
        PreloadRequest::Crossfade { path, gain, speed } => {
            output.stage(wrapped, speed);
            (
                Some(Preload::Crossfade(PreloadedTrack { path, gain, total })),
                Some(control),
            )
        }
    }
}

pub(crate) struct Decoding {
    decode_ticket: Ticket,
    preload_ticket: Ticket,
    preloading: Option<PendingPreload>,
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

    pub(crate) fn spawn_decode(&mut self, path: PathBuf) -> Option<AudioError> {
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
        self.preloading = Some(PendingPreload { request });
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
        landed: (Ticket, Result<TrackDecoder, Error>),
    ) -> Option<Result<Option<Duration>, AudioError>> {
        let (ticket, outcome) = landed;
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

    pub(crate) fn accept_preload(
        &mut self,
        landed: (Ticket, Result<TrackDecoder, Error>),
        output: Option<&mut Output>,
    ) -> Option<Result<Preload, AudioError>> {
        let (ticket, outcome) = landed;
        if ticket != self.preload_ticket {
            return None;
        }
        let pending = self.preloading.take()?;
        match outcome {
            Ok(source) => {
                let total = source.total_duration();
                let decoded = Decoded {
                    source,
                    total,
                    ticket,
                };
                let parts = InstallParts {
                    output,
                    wake: self.wake.clone(),
                };
                let (installed, control) = install(pending.request, decoded, parts);
                match (&installed, control) {
                    (Some(Preload::Gapless(_)), Some(control)) => {
                        self.envelopes.queued = Some(control);
                    }
                    (Some(Preload::Crossfade(_)), Some(control)) => {
                        self.envelopes.incoming = Some(control);
                    }
                    (_, _) => {}
                }
                installed.map(Ok)
            }
            Err(error) => Some(Err(preload_error(&error))),
        }
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
