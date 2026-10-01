use std::{
    ops::ControlFlow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crossbeam_channel::{Receiver, Select, Sender, TrySendError};
use kernel::{
    DriverMessage,
    LibraryEvent,
    Message,
    Outbox,
    SendError,
    domain::{Driver, DriverError},
    update::Machine,
};
use library::{LibraryDirs, execute};

use crate::{
    driver::{DriverThread, spawn_driver},
    error::Error,
    latest::LatestSender,
    library::{
        cover::{CoverDecoded, CoverRequest, DecodeFinished},
        machine::{
            LibraryEffect,
            LibraryMessage,
            LibraryState,
            WatchChange,
            watch_failure,
        },
        worker::CoverWorker,
    },
    registry,
    sender::DriverSender,
    watcher::{Watcher, recommended, rewatch},
};

pub(crate) struct LibraryParts {
    pub(crate) dirs: LibraryDirs,
    pub(crate) decodable: &'static [&'static str],
    pub(crate) cover: LatestSender<CoverDecoded>,
    pub(crate) worker: CoverWorker,
    pub(crate) finished: Receiver<DecodeFinished>,
}

impl LibraryParts {
    pub(crate) fn new(
        dirs: LibraryDirs,
        decodable: &'static [&'static str],
        cover: LatestSender<CoverDecoded>,
    ) -> Result<Self, Error> {
        let (worker, finished) = CoverWorker::spawn(cover.clone())?;
        Ok(Self {
            dirs,
            decodable,
            cover,
            worker,
            finished,
        })
    }
}

pub(crate) fn spawn(
    parts: LibraryParts,
    inbox: &Sender<Message>,
) -> Result<DriverThread<LibraryMessage>, Error> {
    spawn_driver(
        registry::row(Driver::Library),
        move |inbox, outbox| {
            let watching = Watch::recommended(outbox);
            LibraryLoop::new(parts, outbox, watching).run(inbox);
        },
        inbox,
    )
}

enum Wake {
    Command(LibraryMessage),
    FilesystemChange(Result<(), notify::Error>),
    FilesystemEventsLost,
    DebounceElapsed,
    Decoded(DecodeFinished),
    FinishedLost,
    Stopped,
}

#[derive(Debug, Clone, Default)]
struct Overflow(Arc<AtomicBool>);

impl Overflow {
    fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn take_rescan(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

fn watch_events(
    overflow: Overflow,
) -> (
    impl FnMut(notify::Result<notify::Event>) + Send + 'static,
    Receiver<notify::Result<notify::Event>>,
) {
    let (events, filesystem_events) = crossbeam_channel::bounded(64);
    let callback = move |event| match events.try_send(event) {
        Err(TrySendError::Full(_)) => overflow.raise(),
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
    };
    (callback, filesystem_events)
}

struct Watch<W> {
    watcher: W,
    events: Receiver<notify::Result<notify::Event>>,
    overflow: Overflow,
}

impl Watch<Option<notify::RecommendedWatcher>> {
    fn recommended(outbox: &DriverSender<LibraryEvent>) -> Self {
        let overflow = Overflow::default();
        let (callback, events) = watch_events(overflow.clone());
        let watcher =
            recommended(callback, |error| match outbox.send(watch_failure(error)) {
                Ok(()) | Err(SendError::Full | SendError::Closed) => {}
            });
        Self {
            watcher,
            events,
            overflow,
        }
    }
}

struct LibraryLoop<'a, W> {
    driver: LibraryState,
    watcher: W,
    filesystem_events: Receiver<notify::Result<notify::Event>>,
    overflow: Overflow,
    worker: CoverWorker,
    finished: Receiver<DecodeFinished>,
    paths: LibraryDirs,
    decodable: &'static [&'static str],
    outbox: &'a DriverSender<LibraryEvent>,
    cover: LatestSender<CoverDecoded>,
}

impl<'a, W: Watcher> LibraryLoop<'a, W> {
    fn new(
        parts: LibraryParts,
        outbox: &'a DriverSender<LibraryEvent>,
        watching: Watch<W>,
    ) -> Self {
        Self {
            driver: LibraryState::default(),
            watcher: watching.watcher,
            filesystem_events: watching.events,
            overflow: watching.overflow,
            worker: parts.worker,
            finished: parts.finished,
            paths: parts.dirs,
            decodable: parts.decodable,
            outbox,
            cover: parts.cover,
        }
    }

    fn run(mut self, inbox: &Receiver<LibraryMessage>) {
        loop {
            let halt = match self.wait(inbox) {
                Wake::Command(message) => self.feed(message),
                Wake::FilesystemChange(event) => self.filesystem_change(event),
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                    ControlFlow::Continue(())
                }
                Wake::DebounceElapsed => self.feed(LibraryMessage::DebounceDue),
                Wake::Decoded(done) => self.feed(LibraryMessage::Decoded(done)),
                Wake::FinishedLost => {
                    self.finished = crossbeam_channel::never();
                    ControlFlow::Continue(())
                }
                Wake::Stopped => {
                    match self.feed(LibraryMessage::Stopping) {
                        ControlFlow::Continue(()) | ControlFlow::Break(()) => {}
                    }
                    ControlFlow::Break(())
                }
            };
            if halt.is_break() {
                self.retire();
                return;
            }
        }
    }

    fn retire(self) {
        if self.worker.join().is_err() {
            let failure = DriverError::Panicked("cover worker".to_owned());
            let died = DriverMessage::Died(failure);
            match self.outbox.report(Driver::Library, died) {
                Ok(()) | Err(SendError::Full | SendError::Closed) => {}
            }
        }
    }

    fn wait(&self, inbox: &Receiver<LibraryMessage>) -> Wake {
        let mut select = Select::new();
        let command_index = select.recv(inbox);
        let filesystem_index = select.recv(&self.filesystem_events);
        let results_index = select.recv(&self.finished);
        let selected = match self.driver.deadline() {
            Some(deadline) => select.select_deadline(deadline),
            None => Ok(select.select()),
        };
        let Ok(operation) = selected else {
            return Wake::DebounceElapsed;
        };
        if operation.index() == command_index {
            return operation.recv(inbox).map_or(Wake::Stopped, Wake::Command);
        }
        if operation.index() == filesystem_index {
            return operation
                .recv(&self.filesystem_events)
                .map_or(Wake::FilesystemEventsLost, |event| {
                    Wake::FilesystemChange(event.map(|_| ()))
                });
        }
        if operation.index() == results_index {
            return operation
                .recv(&self.finished)
                .map_or(Wake::FinishedLost, Wake::Decoded);
        }
        Wake::Stopped
    }

    fn filesystem_change(
        &mut self,
        event: Result<(), notify::Error>,
    ) -> ControlFlow<()> {
        let halt = self.feed(LibraryMessage::FilesChanged {
            at: Instant::now(),
            event,
        });
        if halt.is_continue() && self.overflow.take_rescan() {
            return self.feed(LibraryMessage::EventsOverflowed { at: Instant::now() });
        }
        halt
    }

    fn feed(&mut self, input: LibraryMessage) -> ControlFlow<()> {
        let label: &'static str = (&input).into();
        match self.driver.update(input) {
            Ok(outputs) => self.act_all(outputs),
            Err(_) => self.reject(label),
        }
    }

    fn act_all(&mut self, outputs: Vec<LibraryEffect>) -> ControlFlow<()> {
        outputs.into_iter().try_for_each(|output| self.act(output))
    }

    fn reject(&self, input: &'static str) -> ControlFlow<()> {
        let rejected = DriverMessage::Rejected { input };
        match self.outbox.report(Driver::Library, rejected) {
            Err(SendError::Closed) => ControlFlow::Break(()),
            Ok(()) | Err(SendError::Full) => ControlFlow::Continue(()),
        }
    }

    fn act(&mut self, output: LibraryEffect) -> ControlFlow<()> {
        match output {
            LibraryEffect::Execute(command) => self.execute(command),
            LibraryEffect::Watch(change) => self.watch_change(change),
            LibraryEffect::Decode(request) => self.decode(request),
            LibraryEffect::Publish(decoded) => self.publish(decoded),
            LibraryEffect::Event(event) => self.emit(event),
        }
    }

    fn execute(&mut self, command: kernel::LibraryCmd) -> ControlFlow<()> {
        let result = execute(command, &self.paths, self.decodable);
        self.feed(LibraryMessage::Executed(result))
    }

    fn watch_change(&mut self, change: WatchChange) -> ControlFlow<()> {
        let outcome = match change {
            WatchChange::Watch(music_dir) => self.watcher.watch(&music_dir),
            WatchChange::Rewatch { from, to } => rewatch(&mut self.watcher, &from, &to),
        };
        match outcome {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => self.emit(watch_failure(&error)),
        }
    }

    fn decode(&self, request: CoverRequest) -> ControlFlow<()> {
        self.worker.request(request);
        ControlFlow::Continue(())
    }

    fn publish(&self, decoded: CoverDecoded) -> ControlFlow<()> {
        self.cover.publish(decoded);
        ControlFlow::Continue(())
    }

    fn emit(&self, event: LibraryEvent) -> ControlFlow<()> {
        match self.outbox.send(event) {
            Err(SendError::Closed) => ControlFlow::Break(()),
            Ok(()) | Err(SendError::Full) => ControlFlow::Continue(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crossbeam_channel::{Receiver, SendError};
    use kernel::{
        DriverMessage,
        LibraryCmd,
        LibraryError,
        LibraryEvent,
        LibrarySubject,
        Message,
        cmd::ScanMode,
        domain::{Driver, DriverError, Revision},
    };
    use library::LibraryDirs;

    use crate::{
        driver::DriverThread,
        latest::latest_channels,
        library::{
            cover::{CoverDecoded, CoverOutcome, CoverRequest},
            driver::{LibraryLoop, LibraryParts, Overflow, Wake, Watch, spawn},
            machine::{LibraryMessage, LibraryState},
            worker::CoverWorker,
        },
        sender::{DriverSender, FullEdge},
        watcher::FakeWatch,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);
    const DECODABLE: &[&str] = &["mp3"];

    fn paths(directory: &tempfile::TempDir) -> LibraryDirs {
        LibraryDirs::under(directory.path())
    }

    fn spawned(
        directory: &tempfile::TempDir,
    ) -> (DriverThread<LibraryMessage>, Receiver<Message>) {
        let (inbox, messages) = crossbeam_channel::unbounded();
        let (writers, _cells, _doorbell) = latest_channels();
        let thread = spawn(
            LibraryParts::new(paths(directory), DECODABLE, writers.cover).unwrap(),
            &inbox,
        )
        .unwrap();
        (thread, messages)
    }

    fn stopped(thread: DriverThread<LibraryMessage>) {
        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        while let Ok(item) = receiver.recv_timeout(SETTLE_TIMEOUT) {
            collected.push(item);
        }
        collected
    }

    fn listed_tracks(message: Message) -> Option<Vec<std::sync::Arc<kernel::Track>>> {
        let Message::Library(LibraryEvent::Listed { tracks, .. }) = message else {
            return None;
        };
        Some(tracks)
    }

    fn scanned_track_count(message: Message) -> Option<usize> {
        let Message::Library(LibraryEvent::Loaded { tracks, .. }) = message else {
            return None;
        };
        Some(tracks.len())
    }

    #[test]
    fn a_scan_library_command_comes_back_as_library_listed() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("one.mp3"), b"stub").unwrap();
        let (thread, messages) = spawned(&directory);
        thread
            .commands
            .send(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: directory.path().to_path_buf(),
                revision: Revision::default(),
                mode: ScanMode::Cached,
            }))
            .unwrap();
        let listed = drain(&messages).into_iter().find_map(listed_tracks);
        assert_eq!(listed.map(|tracks| tracks.len()), Some(1));
        stopped(thread);
    }

    #[test]
    fn a_scan_of_a_missing_root_is_reported_as_a_library_failure() {
        let directory = tempfile::tempdir().unwrap();
        let (thread, messages) = spawned(&directory);
        thread
            .commands
            .send(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: directory.path().join("missing"),
                revision: Revision::default(),
                mode: ScanMode::Cached,
            }))
            .unwrap();
        let failed = drain(&messages).into_iter().any(|message| {
            matches!(
                message,
                Message::Library(LibraryEvent::Error(LibraryError::File {
                    subject: LibrarySubject::Scan,
                    ..
                }))
            )
        });
        assert!(
            failed,
            "a scan of a missing music dir must report a failure"
        );
        stopped(thread);
    }

    #[test]
    fn a_stopped_library_driver_is_reported_through_the_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let (thread, messages) = spawned(&directory);
        stopped(thread);
        let reported = drain(&messages).into_iter().any(|message| {
            matches!(
                message,
                Message::Driver {
                    driver: Driver::Library,
                    ..
                }
            )
        });
        assert!(reported, "the wrapper must report the driver's stop");
    }

    #[test]
    fn the_frame_asks_for_a_cover_once_and_receives_it_decoded() {
        let directory = tempfile::tempdir().unwrap();
        let track = directory.path().join("untagged.mp3");
        std::fs::write(&track, b"stub").unwrap();
        let (inbox, _messages) = crossbeam_channel::unbounded();
        let (writers, cells, doorbell) = latest_channels();
        let thread = spawn(
            LibraryParts::new(paths(&directory), DECODABLE, writers.cover).unwrap(),
            &inbox,
        )
        .unwrap();
        thread
            .commands
            .send(LibraryMessage::Cover(CoverRequest {
                path: track.clone(),
                size_px: 64,
            }))
            .unwrap();
        doorbell.recv_timeout(RECV_TIMEOUT).unwrap();
        let decoded = cells.cover.take().unwrap();
        assert_eq!(decoded.path, track);
        assert!(matches!(decoded.outcome, CoverOutcome::NoArt));
        assert!(
            doorbell.recv_timeout(SETTLE_TIMEOUT).is_err(),
            "one request must answer with exactly one decoded cover"
        );
        stopped(thread);
    }

    #[test]
    fn a_fake_file_event_rescans_after_the_debounce() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("one.mp3"), b"stub").unwrap();
        let (sender, messages) = crossbeam_channel::unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        let parts =
            LibraryParts::new(paths(&directory), DECODABLE, writers.cover).unwrap();
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let mut driver_loop = LibraryLoop::new(parts, &outbox, watching);
        let (_commands, inbox) = crossbeam_channel::unbounded();

        assert!(
            driver_loop
                .feed(LibraryMessage::Cmd(LibraryCmd::Scan {
                    music_dir: directory.path().to_path_buf(),
                    revision: Revision::default().next(),
                    mode: ScanMode::Full,
                }))
                .is_continue()
        );
        assert_eq!(
            messages.try_iter().find_map(scanned_track_count),
            Some(1),
            "the initial rescan must see the seed file"
        );
        assert_eq!(
            driver_loop.watcher.watched,
            vec![directory.path().to_path_buf()]
        );

        for index in 0..3 {
            std::fs::write(directory.path().join(format!("burst{index}.mp3")), b"stub")
                .unwrap();
            events.send(Ok(notify::Event::default())).unwrap();
            let Wake::FilesystemChange(event) = driver_loop.wait(&inbox) else {
                panic!("a fake file event must wake the loop");
            };
            assert!(driver_loop.filesystem_change(event).is_continue());
        }
        assert!(matches!(driver_loop.wait(&inbox), Wake::DebounceElapsed));
        assert!(driver_loop.feed(LibraryMessage::DebounceDue).is_continue());

        assert_eq!(
            messages.try_iter().find_map(scanned_track_count),
            Some(4),
            "the debounced rescan must see every file from the burst"
        );
        assert!(
            messages.try_recv().is_err(),
            "the burst must coalesce into a single rescan"
        );
        driver_loop.retire();
    }

    #[test]
    fn a_lost_filesystem_watcher_becomes_never_without_stopping_the_loop() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, _messages) = crossbeam_channel::unbounded();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        drop(events);
        let (worker, finished) = CoverWorker::spawn(writers.cover.clone()).unwrap();
        let mut driver_loop = LibraryLoop {
            driver: LibraryState::default(),
            watcher: FakeWatch::default(),
            filesystem_events,
            overflow: Overflow::default(),
            worker,
            finished,
            paths: paths(&directory),
            decodable: DECODABLE,
            outbox: &outbox,
            cover: writers.cover,
        };
        let (commands, inbox) = crossbeam_channel::unbounded();
        assert!(matches!(
            driver_loop.wait(&inbox),
            Wake::FilesystemEventsLost
        ));
        driver_loop.filesystem_events = crossbeam_channel::never();
        commands
            .send(LibraryMessage::Cmd(LibraryCmd::LoadFavorites))
            .unwrap();
        assert!(matches!(driver_loop.wait(&inbox), Wake::Command(_)));
        driver_loop.retire();
    }

    #[test]
    fn a_closed_inbox_ends_the_library_driver() {
        let directory = tempfile::tempdir().unwrap();
        let (inbox, messages) = crossbeam_channel::unbounded();
        let (writers, _cells, _doorbell) = latest_channels();
        let thread = spawn(
            LibraryParts::new(paths(&directory), DECODABLE, writers.cover).unwrap(),
            &inbox,
        )
        .unwrap();

        drop(messages);

        thread
            .commands
            .send(LibraryMessage::Cmd(LibraryCmd::LoadFavorites))
            .unwrap();

        let report = thread.handle.join().unwrap();
        assert_eq!(
            report,
            Err(SendError(Message::Driver {
                driver: Driver::Library,
                event: DriverMessage::Stopped
            }))
        );
    }

    #[test]
    fn a_rejected_input_is_reported_to_the_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = crossbeam_channel::unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let (_events, filesystem_events) = crossbeam_channel::unbounded();
        let parts =
            LibraryParts::new(paths(&directory), DECODABLE, writers.cover).unwrap();
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let mut driver_loop = LibraryLoop::new(parts, &outbox, watching);

        let halt = driver_loop.feed(LibraryMessage::DebounceDue);

        assert!(halt.is_continue());
        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver {
                driver: Driver::Library,
                event: DriverMessage::Rejected {
                    input: "debounce_due"
                }
            }]
        );
        assert!(driver_loop.feed(LibraryMessage::Stopping).is_continue());
        driver_loop.retire();
    }

    #[test]
    fn a_panicking_cover_worker_is_reported_on_stop() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = crossbeam_channel::unbounded::<Message>();
        let outbox = DriverSender::new(sender, FullEdge::default());
        let (writers, _cells, _doorbell) = latest_channels();
        let (_events, filesystem_events) = crossbeam_channel::unbounded();
        let (worker, finished) = CoverWorker::spawn_with(
            writers.cover.clone(),
            |_request: &CoverRequest| -> CoverDecoded { panic!("decode blew up") },
        )
        .unwrap();
        let parts = LibraryParts {
            dirs: paths(&directory),
            decodable: DECODABLE,
            cover: writers.cover,
            worker,
            finished,
        };
        let watching = Watch {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let driver_loop = LibraryLoop::new(parts, &outbox, watching);
        driver_loop.worker.request(CoverRequest {
            path: PathBuf::from("a"),
            size_px: 64,
        });

        driver_loop.retire();

        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver {
                driver: Driver::Library,
                event: DriverMessage::Died(DriverError::Panicked(
                    "cover worker".to_owned()
                ))
            }]
        );
    }
}
