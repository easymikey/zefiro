use std::{
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, SendError, Sender, TryRecvError, TrySendError};

use crate::{
    deck::{
        DeckEvent,
        Ticket,
        source::{DecodeResult, decode},
    },
    device::list_output_devices,
    error::AudioError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Decode,
    Preload,
}

pub(crate) struct DecodeRequest {
    pub(crate) path: PathBuf,
    pub(crate) ticket: Ticket,
}

pub(crate) enum Job {
    Decode(DecodeRequest),
    Preload(DecodeRequest),
    ListDevices,
}

pub(crate) struct Slot<T> {
    sender: Sender<T>,
    drain: Receiver<T>,
}

impl<T> Slot<T> {
    fn new() -> Self {
        let (sender, drain) = crossbeam_channel::bounded(1);
        Self { sender, drain }
    }

    pub(crate) fn replace(&self, value: T) {
        let _ = self.drain.try_recv();
        if let Err(TrySendError::Full(value)) = self.sender.try_send(value) {
            let _ = self.drain.try_recv();
            match self.sender.try_send(value) {
                Ok(()) | Err(TrySendError::Disconnected(_) | TrySendError::Full(_)) => {
                }
            }
        }
    }

    fn close(&mut self) {
        let (sender, _) = crossbeam_channel::bounded(0);
        self.sender = sender;
    }
}

pub(crate) struct DecodeWorker {
    decode: Slot<DecodeRequest>,
    preload: Slot<DecodeRequest>,
    devices: Slot<()>,
    handle: Option<JoinHandle<Result<(), SendError<DeckEvent>>>>,
}

struct WorkerChannels {
    decode: Receiver<DecodeRequest>,
    preload: Receiver<DecodeRequest>,
    devices: Receiver<()>,
    wake: Sender<DeckEvent>,
}

impl DecodeWorker {
    pub(crate) fn spawn(wake: Sender<DeckEvent>) -> Result<Self, AudioError> {
        let decode = Slot::new();
        let preload = Slot::new();
        let devices = Slot::new();
        let channels = WorkerChannels {
            decode: decode.drain.clone(),
            preload: preload.drain.clone(),
            devices: devices.drain.clone(),
            wake,
        };
        let builder = thread::Builder::new().name("audio-worker".into());
        let handle = builder
            .spawn(move || serve(&channels))
            .map_err(AudioError::Spawn)?;
        Ok(Self {
            decode,
            preload,
            devices,
            handle: Some(handle),
        })
    }

    pub(crate) fn submit(&self, job: Job) {
        match job {
            Job::Decode(request) => self.decode.replace(request),
            Job::Preload(request) => self.preload.replace(request),
            Job::ListDevices => self.devices.replace(()),
        }
    }
}

impl Drop for DecodeWorker {
    fn drop(&mut self) {
        self.decode.close();
        self.preload.close();
        self.devices.close();
        if let Some(handle) = self.handle.take() {
            join_quietly(handle);
        }
    }
}

fn join_quietly(handle: JoinHandle<Result<(), SendError<DeckEvent>>>) {
    drop(handle.join());
}

fn disconnected<T>(receiver: &Receiver<T>) -> bool {
    matches!(receiver.try_recv(), Err(TryRecvError::Disconnected))
}

fn serve(channels: &WorkerChannels) -> Result<(), SendError<DeckEvent>> {
    let WorkerChannels {
        decode,
        preload,
        devices,
        wake,
    } = channels;
    loop {
        crossbeam_channel::select! {
            recv(decode) -> request => match request {
                Ok(request) => decode_job(request, Role::Decode, wake)?,
                Err(_) if disconnected(preload) && disconnected(devices) => break,
                Err(_) => {}
            },
            recv(preload) -> request => match request {
                Ok(request) => {
                    if let Ok(primary) = decode.try_recv() {
                        decode_job(primary, Role::Decode, wake)?;
                    }
                    decode_job(request, Role::Preload, wake)?;
                }
                Err(_) if disconnected(decode) && disconnected(devices) => break,
                Err(_) => {}
            },
            recv(devices) -> signal => match signal {
                Ok(()) => list_devices(wake)?,
                Err(_) if disconnected(decode) && disconnected(preload) => break,
                Err(_) => {}
            },
        }
    }
    Ok(())
}

fn decode_job(
    request: DecodeRequest,
    role: Role,
    wake: &Sender<DeckEvent>,
) -> Result<(), SendError<DeckEvent>> {
    let DecodeRequest { path, ticket } = request;
    let outcome: DecodeResult = panic::catch_unwind(AssertUnwindSafe(|| decode(&path)))
        .unwrap_or_else(|_panic| Err(AudioError::WorkerPanicked { path }));
    let event = match role {
        Role::Decode => DeckEvent::Decoded { ticket, outcome },
        Role::Preload => DeckEvent::Preloaded { ticket, outcome },
    };
    wake.send(event)
}

fn list_devices(wake: &Sender<DeckEvent>) -> Result<(), SendError<DeckEvent>> {
    wake.send(DeckEvent::DevicesListed(list_output_devices()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        path::PathBuf,
        process::Command,
        time::{Duration, Instant},
    };

    use crossbeam_channel::TryRecvError;

    use crate::deck::{
        Ticket,
        worker::{DecodeRequest, DecodeWorker, Job, Slot},
    };

    fn never_opening_file() -> Option<PathBuf> {
        let path =
            std::env::temp_dir().join(format!("sifr-preload-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let made = Command::new("mkfifo").arg(&path).status().ok()?;
        made.success().then_some(path)
    }

    #[test]
    fn a_replaced_request_is_never_opened() {
        let slot = Slot::new();
        slot.replace(DecodeRequest {
            path: PathBuf::from("/first"),
            ticket: Ticket::default(),
        });
        slot.replace(DecodeRequest {
            path: PathBuf::from("/second"),
            ticket: Ticket::default(),
        });

        let received = slot.drain.try_recv().unwrap();
        assert_eq!(received.path, PathBuf::from("/second"));
        assert!(matches!(slot.drain.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn a_dropped_worker_joins() {
        let (wake, _heard) = crossbeam_channel::bounded(1);
        let worker = DecodeWorker::spawn(wake).unwrap();
        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_dropped_worker_still_ends_with_a_job_in_flight() {
        let (wake, _heard) = crossbeam_channel::bounded(1);
        let worker = DecodeWorker::spawn(wake).unwrap();
        worker.submit(Job::Decode(DecodeRequest {
            path: PathBuf::from("/no/such/track"),
            ticket: Ticket::default(),
        }));

        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_preload_on_a_file_that_never_opens_still_returns_at_once() {
        let Some(path) = never_opening_file() else {
            return;
        };
        let (wake, heard) = crossbeam_channel::bounded(1);
        let worker = DecodeWorker::spawn(wake).unwrap();

        let started = Instant::now();
        worker.submit(Job::Preload(DecodeRequest {
            path: path.clone(),
            ticket: Ticket::default(),
        }));
        let landed = heard.try_recv();
        let waited = started.elapsed();

        let unblock = path.clone();
        let opened = std::thread::Builder::new()
            .spawn(move || {
                let _ = File::create(&unblock);
            })
            .unwrap();
        opened.join().unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(matches!(landed, Err(TryRecvError::Empty)));
        assert!(
            waited < Duration::from_secs(1),
            "starting a preload must not wait on the file, waited {waited:?}"
        );
    }
}
