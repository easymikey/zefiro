use std::{
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    thread,
    thread::JoinHandle,
    time::Duration,
};

use kernel::AudioFailure;
use rodio::Source;

use crate::{
    deck::output::Output,
    error::{AudioError, preload_fault},
};

pub(crate) type TrackDecoder = rodio::Decoder<BufReader<File>>;
type DecodeResult = Result<TrackDecoder, AudioError>;

const READ_CAPACITY: usize = 1 << 20;

struct PendingDecode {
    path: PathBuf,
    handle: Result<JoinHandle<DecodeResult>, AudioError>,
}

pub(crate) enum PreloadRequest {
    Gapless(PathBuf),
    Crossfade {
        path: PathBuf,
        gain: Option<f32>,
        speed: f32,
    },
}

pub(crate) enum Landed {
    Gapless(PathBuf),
    Crossfade {
        path: PathBuf,
        gain: Option<f32>,
        total: Option<Duration>,
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
    handle: Result<JoinHandle<DecodeResult>, AudioError>,
}

fn decode(path: &Path) -> DecodeResult {
    let file = File::open(path).map_err(|error| AudioError::Decode {
        path: path.to_path_buf(),
        source: rodio::decoder::DecoderError::IoError(error.to_string()),
    })?;
    let length = file
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    let reader = BufReader::with_capacity(READ_CAPACITY, file);
    let builder = rodio::Decoder::builder()
        .with_data(reader)
        .with_byte_len(length)
        .with_seekable(true);
    let builder = match path.extension().and_then(OsStr::to_str) {
        Some(hint) => builder.with_hint(hint),
        None => builder,
    };
    builder.build().map_err(|source| AudioError::Decode {
        path: path.to_path_buf(),
        source,
    })
}

fn install(
    request: PreloadRequest,
    decoded: (TrackDecoder, Option<Duration>),
    output: Option<&mut Output>,
) -> Option<Landed> {
    let (source, total) = decoded;
    match request {
        PreloadRequest::Gapless(path) => {
            output?.append(source);
            Some(Landed::Gapless(path))
        }
        PreloadRequest::Crossfade { path, gain, speed } => {
            output?.stage(source, speed);
            Some(Landed::Crossfade { path, gain, total })
        }
    }
}

fn spawn_worker(path: PathBuf) -> Result<JoinHandle<DecodeResult>, AudioError> {
    thread::Builder::new()
        .name("audio-decode".into())
        .spawn(move || decode(&path))
        .map_err(AudioError::Spawn)
}

pub(crate) struct DeckSource {
    decode: Option<PendingDecode>,
    preloading: Option<PendingPreload>,
    staged: Option<TrackDecoder>,
}

impl DeckSource {
    pub(crate) fn new() -> Self {
        Self {
            decode: None,
            preloading: None,
            staged: None,
        }
    }

    pub(crate) fn spawn_decode(&mut self, path: PathBuf) {
        let handle = spawn_worker(path.clone());
        self.staged = None;
        self.decode = Some(PendingDecode { path, handle });
    }

    pub(crate) fn start_preload(&mut self, request: PreloadRequest) {
        let handle = spawn_worker(request.path().to_path_buf());
        self.preloading = Some(PendingPreload { request, handle });
    }

    pub(crate) fn drop_preload(&mut self) {
        self.preloading = None;
    }

    pub(crate) fn clear_staged(&mut self) {
        self.decode = None;
        self.staged = None;
    }

    pub(crate) fn poll_decode(
        &mut self,
    ) -> Option<Result<Option<Duration>, AudioFailure>> {
        let pending = self.decode.take()?;
        let handle = match pending.handle {
            Err(error) => return Some(Err(AudioFailure::from(&error))),
            Ok(handle) => handle,
        };
        if !handle.is_finished() {
            self.decode = Some(PendingDecode {
                path: pending.path,
                handle: Ok(handle),
            });
            return None;
        }
        match handle.join() {
            Ok(Ok(source)) => {
                let total = source.total_duration();
                self.staged = Some(source);
                Some(Ok(total))
            }
            Ok(Err(error)) => Some(Err(AudioFailure::from(&error))),
            Err(_panic) => Some(Err(AudioFailure::Decode {
                path: pending.path,
                reason: "the decode worker panicked".to_owned(),
            })),
        }
    }

    pub(crate) fn poll_preload(
        &mut self,
        output: Option<&mut Output>,
    ) -> Option<Result<Landed, AudioFailure>> {
        let pending = self.preloading.take()?;
        let handle = match pending.handle {
            Err(error) => return Some(Err(preload_fault(&error))),
            Ok(handle) => handle,
        };
        if !handle.is_finished() {
            self.preloading = Some(PendingPreload {
                request: pending.request,
                handle: Ok(handle),
            });
            return None;
        }
        match handle.join() {
            Ok(Ok(source)) => {
                let total = source.total_duration();
                install(pending.request, (source, total), output).map(Ok)
            }
            Ok(Err(error)) => Some(Err(preload_fault(&error))),
            Err(_panic) => Some(Err(AudioFailure::Preload {
                path: pending.request.path().to_path_buf(),
                reason: "the decode worker panicked".to_owned(),
            })),
        }
    }

    pub(crate) fn append_staged(&mut self, output: Option<&mut Output>) {
        let Some(source) = self.staged.take() else {
            return;
        };
        if let Some(output) = output {
            output.append(source);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        path::PathBuf,
        process::Command,
        time::{Duration, Instant},
    };

    use crate::deck::source::{DeckSource, PreloadRequest};

    fn never_opening_file() -> Option<PathBuf> {
        let path =
            std::env::temp_dir().join(format!("sifr-preload-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let made = Command::new("mkfifo").arg(&path).status().ok()?;
        made.success().then_some(path)
    }

    fn silent_source() -> DeckSource {
        DeckSource::new()
    }

    #[test]
    fn a_preload_on_a_file_that_never_opens_still_returns_at_once() {
        let Some(path) = never_opening_file() else {
            return;
        };
        let mut source = silent_source();

        let started = Instant::now();
        source.start_preload(PreloadRequest::Gapless(path.clone()));
        let landed = source.poll_preload(None);
        let waited = started.elapsed();

        let unblock = path.clone();
        let _ = std::thread::Builder::new().spawn(move || {
            let _ = File::create(&unblock);
        });
        let _ = std::fs::remove_file(&path);

        assert!(landed.is_none());
        assert!(
            waited < Duration::from_secs(1),
            "starting a preload must not wait on the file, waited {waited:?}"
        );
    }
}
