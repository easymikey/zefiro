use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use arc_swap::ArcSwapOption;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use kernel::domain::DriverName;

use crate::{
    error::Error,
    latest::LatestSender,
    library::cover::{
        CachedOutcome,
        CoverDecoded,
        CoverRequest,
        DecodeFinished,
        decode,
    },
};

pub(crate) struct CoverWorker {
    pending: Arc<ArcSwapOption<CoverRequest>>,
    notify: Sender<()>,
    handle: JoinHandle<()>,
}

impl CoverWorker {
    pub(crate) fn spawn(
        cover: LatestSender<CoverDecoded>,
    ) -> Result<(Self, Receiver<DecodeFinished>), Error> {
        Self::spawn_with(cover, decode)
    }

    pub(crate) fn spawn_with<Decode>(
        cover: LatestSender<CoverDecoded>,
        decode: Decode,
    ) -> Result<(Self, Receiver<DecodeFinished>), Error>
    where
        Decode: Fn(&CoverRequest) -> CoverDecoded + Send + 'static,
    {
        let pending: Arc<ArcSwapOption<CoverRequest>> =
            Arc::new(ArcSwapOption::empty());
        let (notify, wake) = crossbeam_channel::bounded(1);
        let (finished, done) = crossbeam_channel::unbounded();
        let worker_pending = Arc::clone(&pending);
        let handle = thread::Builder::new()
            .name("sifr-cover".to_owned())
            .spawn(move || {
                CoverLoop {
                    wake,
                    pending: worker_pending,
                    cover,
                    finished,
                    decode,
                }
                .run();
            })
            .map_err(|source| Error::Spawn {
                driver: DriverName::Library,
                source,
            })?;
        Ok((
            Self {
                pending,
                notify,
                handle,
            },
            done,
        ))
    }

    pub(crate) fn request(&self, request: CoverRequest) {
        self.pending.store(Some(Arc::new(request)));
        match self.notify.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => self.pending.store(None),
        }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        drop(self.notify);
        self.handle.join()
    }
}

struct CoverLoop<Decode> {
    wake: Receiver<()>,
    pending: Arc<ArcSwapOption<CoverRequest>>,
    cover: LatestSender<CoverDecoded>,
    finished: Sender<DecodeFinished>,
    decode: Decode,
}

impl<Decode> CoverLoop<Decode>
where
    Decode: Fn(&CoverRequest) -> CoverDecoded,
{
    fn run(self) {
        while self.wake.recv().is_ok() {
            let Some(request) = self.pending.swap(None) else {
                continue;
            };
            let decoded = (self.decode)(&request);
            let done = DecodeFinished {
                path: decoded.path.clone(),
                side: decoded.side,
                cached: CachedOutcome::from_outcome(&decoded.outcome),
            };
            self.cover.publish(decoded);
            if self.finished.send(done).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::{
        latest::latest_channels,
        library::{
            cover::{CoverDecoded, CoverOutcome, CoverRequest},
            worker::CoverWorker,
        },
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    #[test]
    fn a_newer_cover_request_replaces_a_pending_one() {
        let (writers, _cells, _notified) = latest_channels();
        let (release, wait) = crossbeam_channel::bounded(0);
        let (started, entered) = crossbeam_channel::unbounded();
        let decode_fn = move |request: &CoverRequest| {
            started.send(request.path.clone()).unwrap();
            wait.recv().unwrap();
            CoverDecoded {
                path: request.path.clone(),
                side: request.size_px,
                outcome: CoverOutcome::NoArt,
            }
        };
        let (worker, _results) =
            CoverWorker::spawn_with(writers.cover, decode_fn).unwrap();
        let request = |name: &str| CoverRequest {
            path: PathBuf::from(name),
            size_px: 64,
        };

        worker.request(request("a"));
        assert_eq!(
            entered.recv_timeout(RECV_TIMEOUT).unwrap(),
            PathBuf::from("a")
        );
        worker.request(request("b"));
        worker.request(request("c"));
        release.send(()).unwrap();
        assert_eq!(
            entered.recv_timeout(RECV_TIMEOUT).unwrap(),
            PathBuf::from("c")
        );
        release.send(()).unwrap();
        assert!(entered.recv_timeout(SETTLE_TIMEOUT).is_err());

        worker.join().unwrap();
    }

    #[test]
    fn a_panicking_decode_fails_the_join() {
        let (writers, _cells, _notified) = latest_channels();
        let (worker, _results) = CoverWorker::spawn_with(
            writers.cover,
            |_request: &CoverRequest| -> CoverDecoded { panic!("decode blew up") },
        )
        .unwrap();

        worker.request(CoverRequest {
            path: PathBuf::from("a"),
            size_px: 64,
        });

        assert!(worker.join().is_err());
    }
}
