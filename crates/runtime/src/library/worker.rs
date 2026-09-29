use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use arc_swap::ArcSwapOption;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use kernel::domain::Driver;

use crate::{
    cells::Latest,
    error::RuntimeError,
    library::cover::{CachedOutcome, CoverDecoded, CoverDone, CoverRequest, decode},
};

pub(crate) struct CoverWorker {
    pending: Arc<ArcSwapOption<CoverRequest>>,
    doorbell: Sender<()>,
    handle: JoinHandle<()>,
}

impl CoverWorker {
    pub(crate) fn spawn(
        cover: Latest<CoverDecoded>,
    ) -> Result<(Self, Receiver<CoverDone>), RuntimeError> {
        Self::spawn_with(cover, decode)
    }

    pub(crate) fn spawn_with<Decode>(
        cover: Latest<CoverDecoded>,
        decode: Decode,
    ) -> Result<(Self, Receiver<CoverDone>), RuntimeError>
    where
        Decode: Fn(&CoverRequest) -> CoverDecoded + Send + 'static,
    {
        let pending: Arc<ArcSwapOption<CoverRequest>> =
            Arc::new(ArcSwapOption::empty());
        let (doorbell, wake) = crossbeam_channel::bounded(1);
        let (results, done) = crossbeam_channel::unbounded();
        let worker_pending = Arc::clone(&pending);
        let handle = thread::Builder::new()
            .name("sifr-cover".to_owned())
            .spawn(move || {
                CoverLoop {
                    wake,
                    pending: worker_pending,
                    cover,
                    results,
                    decode,
                }
                .run();
            })
            .map_err(|source| RuntimeError::Spawn {
                driver: Driver::Library,
                source,
            })?;
        Ok((
            Self {
                pending,
                doorbell,
                handle,
            },
            done,
        ))
    }

    pub(crate) fn request(&self, request: CoverRequest) {
        self.pending.store(Some(Arc::new(request)));
        match self.doorbell.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => self.pending.store(None),
        }
    }

    pub(crate) fn join(self) -> Result<(), CoverPanicked> {
        drop(self.doorbell);
        self.handle.join().map_err(|_| CoverPanicked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoverPanicked;

struct CoverLoop<Decode> {
    wake: Receiver<()>,
    pending: Arc<ArcSwapOption<CoverRequest>>,
    cover: Latest<CoverDecoded>,
    results: Sender<CoverDone>,
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
            let done = CoverDone {
                path: decoded.path.clone(),
                side: decoded.side,
                cached: CachedOutcome::from_outcome(&decoded.outcome),
            };
            self.cover.publish(decoded);
            if self.results.send(done).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::{
        cells::cells,
        library::{
            cover::{CoverDecoded, CoverOutcome, CoverRequest},
            worker::{CoverPanicked, CoverWorker},
        },
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    #[test]
    fn a_newer_cover_request_replaces_a_pending_one() {
        let (writers, _cells, _doorbell) = cells();
        let (release, wait) = crossbeam_channel::bounded(0);
        let (started, entered) = crossbeam_channel::unbounded();
        let decode_fn = move |request: &CoverRequest| {
            started.send(request.path.clone()).unwrap();
            wait.recv().unwrap();
            CoverDecoded {
                path: request.path.clone(),
                side: request.side,
                outcome: CoverOutcome::NoArt,
            }
        };
        let (worker, _results) =
            CoverWorker::spawn_with(writers.cover, decode_fn).unwrap();
        let request = |name: &str| CoverRequest {
            path: PathBuf::from(name),
            side: 64,
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
        let (writers, _cells, _doorbell) = cells();
        let (worker, _results) = CoverWorker::spawn_with(
            writers.cover,
            |_request: &CoverRequest| -> CoverDecoded { panic!("decode blew up") },
        )
        .unwrap();

        worker.request(CoverRequest {
            path: PathBuf::from("a"),
            side: 64,
        });

        assert_eq!(worker.join(), Err(CoverPanicked));
    }
}
