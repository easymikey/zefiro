use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crossbeam_channel::{Receiver, Select, Sender, TrySendError};
use kernel::{
    Delivery,
    DriverMessage,
    LibraryFact,
    Message,
    Outbox,
    domain::{Driver, DriverFailure},
    update::Machine,
};
use library::{LibraryPaths, execute};

use crate::{
    cells::Latest,
    config::watcher::FileWatch,
    driver::{DriverThread, spawn_driver},
    error::RuntimeError,
    interpret::LibraryCommand,
    library::{
        cover::{CoverDecoded, CoverDone, CoverRequest},
        machine::{
            LibraryDriver,
            LibraryInput,
            LibraryOutput,
            WatchChange,
            watch_failure,
        },
        notify::{register, relocate},
        worker::CoverWorker,
    },
    mailbox::Mailbox,
    registry,
};

pub(crate) fn spawn(
    library: (LibraryPaths, &'static [&'static str]),
    mailbox: &Sender<Message>,
    cover: Latest<CoverDecoded>,
) -> Result<DriverThread<LibraryCommand>, RuntimeError> {
    let (paths, decodable) = library;
    let (worker, results) = CoverWorker::spawn(cover.clone())?;
    spawn_driver(
        registry::row(Driver::Library),
        move |inbox, mailbox| {
            let outbound = Outbound {
                mailbox,
                worker,
                results,
                cover,
            };
            let watching = Watching::recommended(outbound.mailbox);
            LibraryLoop::new((paths, decodable), outbound, watching).run(inbox);
        },
        mailbox,
    )
}

struct Outbound<'a> {
    mailbox: &'a Mailbox<LibraryFact>,
    worker: CoverWorker,
    results: Receiver<CoverDone>,
    cover: Latest<CoverDecoded>,
}

enum Wake {
    Command(LibraryCommand),
    FilesystemChange(Result<(), notify::Error>),
    FilesystemEventsLost,
    DebounceElapsed,
    Decoded(CoverDone),
    ResultsLost,
    Stopped,
}

enum Halt {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rescan {
    Needed,
    Settled,
}

#[derive(Debug, Clone, Default)]
struct Overflow(Arc<AtomicBool>);

impl Overflow {
    fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn settle(&self) -> Rescan {
        if self.0.swap(false, Ordering::AcqRel) {
            Rescan::Needed
        } else {
            Rescan::Settled
        }
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

struct Watching<W> {
    watcher: W,
    events: Receiver<notify::Result<notify::Event>>,
    overflow: Overflow,
}

impl Watching<Option<notify::RecommendedWatcher>> {
    fn recommended(mailbox: &Mailbox<LibraryFact>) -> Self {
        let overflow = Overflow::default();
        let (callback, events) = watch_events(overflow.clone());
        let watcher = notify::recommended_watcher(callback).map_or_else(
            |error| {
                match mailbox.send(watch_failure(&error)) {
                    Delivery::Sent | Delivery::Congested | Delivery::Closed => {}
                }
                None
            },
            Some,
        );
        Self {
            watcher,
            events,
            overflow,
        }
    }
}

struct LibraryLoop<'a, W> {
    driver: LibraryDriver,
    watcher: W,
    filesystem_events: Receiver<notify::Result<notify::Event>>,
    overflow: Overflow,
    worker: CoverWorker,
    results: Receiver<CoverDone>,
    paths: LibraryPaths,
    decodable: &'static [&'static str],
    mailbox: &'a Mailbox<LibraryFact>,
    cover: Latest<CoverDecoded>,
}

impl<'a, W: FileWatch> LibraryLoop<'a, W> {
    fn new(
        library: (LibraryPaths, &'static [&'static str]),
        outbound: Outbound<'a>,
        watching: Watching<W>,
    ) -> Self {
        let (paths, decodable) = library;
        Self {
            driver: LibraryDriver::default(),
            watcher: watching.watcher,
            filesystem_events: watching.events,
            overflow: watching.overflow,
            worker: outbound.worker,
            results: outbound.results,
            paths,
            decodable,
            mailbox: outbound.mailbox,
            cover: outbound.cover,
        }
    }

    fn run(mut self, inbox: &Receiver<LibraryCommand>) {
        loop {
            let halt = match self.wait(inbox) {
                Wake::Command(command) => self.command(command),
                Wake::FilesystemChange(event) => self.filesystem_change(event),
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                    Halt::Continue
                }
                Wake::DebounceElapsed => self.feed(LibraryInput::DebounceDue),
                Wake::Decoded(done) => self.feed(LibraryInput::Decoded(done)),
                Wake::ResultsLost => {
                    self.results = crossbeam_channel::never();
                    Halt::Continue
                }
                Wake::Stopped => {
                    self.feed(LibraryInput::Stopping);
                    self.retire();
                    return;
                }
            };
            if let Halt::Stop = halt {
                self.retire();
                return;
            }
        }
    }

    fn retire(self) {
        if self.worker.join().is_err() {
            let failure = DriverFailure::Panicked("cover worker".to_owned());
            let died = DriverMessage::Died(failure);
            match self.mailbox.report(Driver::Library, died) {
                Delivery::Sent | Delivery::Congested | Delivery::Closed => {}
            }
        }
    }

    fn wait(&self, inbox: &Receiver<LibraryCommand>) -> Wake {
        let mut select = Select::new();
        let command_index = select.recv(inbox);
        let filesystem_index = select.recv(&self.filesystem_events);
        let results_index = select.recv(&self.results);
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
                .recv(&self.results)
                .map_or(Wake::ResultsLost, Wake::Decoded);
        }
        Wake::Stopped
    }

    fn command(&mut self, command: LibraryCommand) -> Halt {
        match command {
            LibraryCommand::Kernel(command) => {
                self.feed(LibraryInput::Command(command))
            }
            LibraryCommand::Cover(request) => self.feed(LibraryInput::Cover(request)),
        }
    }

    fn filesystem_change(&mut self, event: Result<(), notify::Error>) -> Halt {
        let halt = self.feed(LibraryInput::FilesChanged {
            at: Instant::now(),
            event,
        });
        if let (Halt::Continue, Rescan::Needed) = (&halt, self.overflow.settle()) {
            return self.feed(LibraryInput::EventsOverflowed { at: Instant::now() });
        }
        halt
    }

    fn feed(&mut self, input: LibraryInput) -> Halt {
        let label: &'static str = (&input).into();
        match self.driver.update(input) {
            Ok(outputs) => self.act_all(outputs),
            Err(_) => self.reject(label),
        }
    }

    fn act_all(&mut self, outputs: Vec<LibraryOutput>) -> Halt {
        for output in outputs {
            if let Halt::Stop = self.act(output) {
                return Halt::Stop;
            }
        }
        Halt::Continue
    }

    fn reject(&self, input: &'static str) -> Halt {
        let rejected = DriverMessage::Rejected { input };
        match self.mailbox.report(Driver::Library, rejected) {
            Delivery::Closed => Halt::Stop,
            Delivery::Sent | Delivery::Congested => Halt::Continue,
        }
    }

    fn act(&mut self, output: LibraryOutput) -> Halt {
        match output {
            LibraryOutput::Execute(command) => self.execute(command),
            LibraryOutput::Watch(change) => self.watch_change(change),
            LibraryOutput::Decode(request) => self.decode(request),
            LibraryOutput::Publish(decoded) => self.publish(decoded),
            LibraryOutput::Tell(fact) => self.tell(fact),
        }
    }

    fn execute(&mut self, command: kernel::LibraryCmd) -> Halt {
        let result = execute(command, &self.paths, self.decodable);
        self.feed(LibraryInput::Executed(result))
    }

    fn watch_change(&mut self, change: WatchChange) -> Halt {
        let outcome = match change {
            WatchChange::Register(root) => register(&mut self.watcher, &root),
            WatchChange::Relocate { from, to } => {
                relocate(&mut self.watcher, &from, &to)
            }
        };
        match outcome {
            Ok(()) => Halt::Continue,
            Err(error) => self.tell(watch_failure(&error)),
        }
    }

    fn decode(&self, request: CoverRequest) -> Halt {
        self.worker.request(request);
        Halt::Continue
    }

    fn publish(&self, decoded: CoverDecoded) -> Halt {
        self.cover.publish(decoded);
        Halt::Continue
    }

    fn tell(&self, fact: LibraryFact) -> Halt {
        match self.mailbox.send(fact) {
            Delivery::Closed => Halt::Stop,
            Delivery::Sent | Delivery::Congested => Halt::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    use crossbeam_channel::{Receiver, SendError};
    use kernel::{
        DriverMessage,
        LibraryCmd,
        LibraryFact,
        LibraryFailure,
        LibrarySubject,
        Message,
        domain::{Driver, DriverFailure, Revision},
    };
    use library::LibraryPaths;

    use crate::{
        cells::cells,
        config::watcher::FileWatch,
        driver::DriverThread,
        interpret::LibraryCommand,
        library::{
            cover::{CoverDecoded, CoverOutcome, CoverRequest},
            driver::{Halt, LibraryLoop, Outbound, Overflow, Wake, Watching, spawn},
            machine::{LibraryDriver, LibraryInput},
            worker::CoverWorker,
        },
        mailbox::{Congestion, Mailbox},
    };

    #[derive(Default)]
    struct FakeWatch {
        watched: Vec<PathBuf>,
    }

    impl FileWatch for FakeWatch {
        fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.retain(|watched| watched != path);
            Ok(())
        }
    }

    const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);
    const DECODABLE: &[&str] = &["mp3"];

    fn paths(directory: &tempfile::TempDir) -> LibraryPaths {
        LibraryPaths {
            cache: directory.path().join("cache"),
            data: directory.path().join("data"),
            playlists: directory.path().join("playlists"),
        }
    }

    fn spawned(
        directory: &tempfile::TempDir,
    ) -> (DriverThread<LibraryCommand>, Receiver<Message>) {
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (writers, _cells, _doorbell) = cells();
        let thread =
            spawn((paths(directory), DECODABLE), &mailbox, writers.cover).unwrap();
        (thread, messages)
    }

    fn stopped(thread: DriverThread<LibraryCommand>) {
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
        let Message::Library(LibraryFact::Listed { tracks, .. }) = message else {
            return None;
        };
        Some(tracks)
    }

    fn scanned_track_count(message: Message) -> Option<usize> {
        let Message::Library(LibraryFact::Loaded { tracks, .. }) = message else {
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
            .send(LibraryCommand::Kernel(LibraryCmd::ScanLibrary {
                root: directory.path().to_path_buf(),
                revision: Revision::default(),
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
            .send(LibraryCommand::Kernel(LibraryCmd::ScanLibrary {
                root: directory.path().join("missing"),
                revision: Revision::default(),
            }))
            .unwrap();
        let failed = drain(&messages).into_iter().any(|message| {
            matches!(
                message,
                Message::Library(LibraryFact::Failed(LibraryFailure::File {
                    subject: LibrarySubject::Scan,
                    ..
                }))
            )
        });
        assert!(failed, "a scan of a missing root must report a failure");
        stopped(thread);
    }

    #[test]
    fn a_stopped_library_driver_is_reported_through_the_mailbox() {
        let directory = tempfile::tempdir().unwrap();
        let (thread, messages) = spawned(&directory);
        stopped(thread);
        let reported = drain(&messages)
            .into_iter()
            .any(|message| matches!(message, Message::Driver(Driver::Library, _)));
        assert!(reported, "the wrapper must report the driver's stop");
    }

    #[test]
    fn the_frame_asks_for_a_cover_once_and_receives_it_decoded() {
        let directory = tempfile::tempdir().unwrap();
        let track = directory.path().join("untagged.mp3");
        std::fs::write(&track, b"stub").unwrap();
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let (writers, cells, doorbell) = cells();
        let thread =
            spawn((paths(&directory), DECODABLE), &mailbox, writers.cover).unwrap();
        thread
            .commands
            .send(LibraryCommand::Cover(CoverRequest {
                path: track.clone(),
                side: 64,
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
        let mailbox = Mailbox::new(sender, Congestion::default());
        let (writers, _cells, _doorbell) = cells();
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        let (worker, results) = CoverWorker::spawn(writers.cover.clone()).unwrap();
        let outbound = Outbound {
            mailbox: &mailbox,
            worker,
            results,
            cover: writers.cover,
        };
        let watching = Watching {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let mut driver_loop =
            LibraryLoop::new((paths(&directory), DECODABLE), outbound, watching);
        let (_commands, inbox) = crossbeam_channel::unbounded();

        driver_loop.command(LibraryCommand::Kernel(LibraryCmd::Rescan {
            root: directory.path().to_path_buf(),
            revision: Revision::default().next(),
        }));
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
            driver_loop.filesystem_change(event);
        }
        assert!(matches!(driver_loop.wait(&inbox), Wake::DebounceElapsed));
        driver_loop.feed(LibraryInput::DebounceDue);

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
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let mailbox = Mailbox::new(mailbox, Congestion::default());
        let (writers, _cells, _doorbell) = cells();
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        drop(events);
        let (worker, results) = CoverWorker::spawn(writers.cover.clone()).unwrap();
        let mut driver_loop = LibraryLoop {
            driver: LibraryDriver::default(),
            watcher: FakeWatch::default(),
            filesystem_events,
            overflow: Overflow::default(),
            worker,
            results,
            paths: paths(&directory),
            decodable: DECODABLE,
            mailbox: &mailbox,
            cover: writers.cover,
        };
        let (commands, inbox) = crossbeam_channel::unbounded();
        assert!(matches!(
            driver_loop.wait(&inbox),
            Wake::FilesystemEventsLost
        ));
        driver_loop.filesystem_events = crossbeam_channel::never();
        commands
            .send(LibraryCommand::Kernel(LibraryCmd::LoadFavorites))
            .unwrap();
        assert!(matches!(driver_loop.wait(&inbox), Wake::Command(_)));
        driver_loop.retire();
    }

    #[test]
    fn a_closed_mailbox_ends_the_library_driver() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (writers, _cells, _doorbell) = cells();
        let thread =
            spawn((paths(&directory), DECODABLE), &mailbox, writers.cover).unwrap();

        drop(messages);

        thread
            .commands
            .send(LibraryCommand::Kernel(LibraryCmd::LoadFavorites))
            .unwrap();

        let report = thread.handle.join().unwrap();
        assert_eq!(
            report,
            Err(SendError(Message::Driver(
                Driver::Library,
                DriverMessage::Stopped
            )))
        );
    }

    #[test]
    fn a_rejected_input_is_reported_to_the_mailbox() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = crossbeam_channel::unbounded::<Message>();
        let mailbox = Mailbox::new(sender, Congestion::default());
        let (writers, _cells, _doorbell) = cells();
        let (_events, filesystem_events) = crossbeam_channel::unbounded();
        let (worker, results) = CoverWorker::spawn(writers.cover.clone()).unwrap();
        let outbound = Outbound {
            mailbox: &mailbox,
            worker,
            results,
            cover: writers.cover,
        };
        let watching = Watching {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let mut driver_loop =
            LibraryLoop::new((paths(&directory), DECODABLE), outbound, watching);

        let halt = driver_loop.feed(LibraryInput::DebounceDue);

        assert!(matches!(halt, Halt::Continue));
        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver(
                Driver::Library,
                DriverMessage::Rejected {
                    input: "DebounceDue"
                }
            )]
        );
        assert!(matches!(
            driver_loop.feed(LibraryInput::Stopping),
            Halt::Continue
        ));
        driver_loop.retire();
    }

    #[test]
    fn a_panicking_cover_worker_is_reported_on_stop() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, messages) = crossbeam_channel::unbounded::<Message>();
        let mailbox = Mailbox::new(sender, Congestion::default());
        let (writers, _cells, _doorbell) = cells();
        let (_events, filesystem_events) = crossbeam_channel::unbounded();
        let (worker, results) = CoverWorker::spawn_with(
            writers.cover.clone(),
            |_request: &CoverRequest| -> CoverDecoded { panic!("decode blew up") },
        )
        .unwrap();
        let outbound = Outbound {
            mailbox: &mailbox,
            worker,
            results,
            cover: writers.cover,
        };
        let watching = Watching {
            watcher: FakeWatch::default(),
            events: filesystem_events,
            overflow: Overflow::default(),
        };
        let driver_loop =
            LibraryLoop::new((paths(&directory), DECODABLE), outbound, watching);
        driver_loop.worker.request(CoverRequest {
            path: PathBuf::from("a"),
            side: 64,
        });

        driver_loop.retire();

        assert_eq!(
            messages.try_iter().collect::<Vec<_>>(),
            vec![Message::Driver(
                Driver::Library,
                DriverMessage::Died(DriverFailure::Panicked("cover worker".to_owned()))
            )]
        );
    }
}
