use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, Sender};
use kernel::{
    Delivery,
    IoFault,
    LibraryCmd,
    LibraryFact,
    LibraryFailure,
    LibrarySubject,
    Message,
    Outbox,
    domain::{Driver, Revision},
    update::Machine,
};
use library::{Executed, LibraryPaths, execute};

use crate::{
    driver::{DriverThread, spawn_driver},
    error::RuntimeError,
    library::{
        cover::{
            CoverCache,
            CoverDecoded,
            CoverRequest,
            DecodeIo,
            DecodeMessage,
            Decoding,
            decode,
        },
        notify::{register, relocate},
        watch::{LibraryWatch, LibraryWatchMessage, WatchIo},
    },
    mailbox::Mailbox,
    registry,
};

const DEBOUNCE: Duration = Duration::from_millis(500);

type LibrarySpawn = (
    DriverThread<LibraryCmd>,
    Sender<CoverRequest>,
    Receiver<CoverDecoded>,
);

pub(crate) fn spawn(
    paths: LibraryPaths,
    decodable: &'static [&'static str],
    mailbox: &Sender<Message>,
) -> Result<LibrarySpawn, RuntimeError> {
    let (covers, cover_inbox) = crossbeam_channel::unbounded();
    let (decoded, decoded_events) = crossbeam_channel::unbounded();
    let thread = spawn_driver(
        registry::row(Driver::Library),
        move |inbox, mailbox| {
            let outbound = Outbound {
                mailbox,
                decoded: &decoded,
                cover_inbox,
            };
            LibraryLoop::new(paths, decodable, outbound).run(inbox);
        },
        mailbox,
    )?;
    Ok((thread, covers, decoded_events))
}

struct Outbound<'a> {
    mailbox: &'a Mailbox<LibraryFact>,
    decoded: &'a Sender<CoverDecoded>,
    cover_inbox: Receiver<CoverRequest>,
}

enum Wake {
    Command(LibraryCmd),
    FilesystemChange(Result<(), notify::Error>),
    FilesystemEventsLost,
    DebounceElapsed,
    Cover(CoverRequest),
    CoverInboxLost,
    Stopped,
}

#[derive(Clone, Copy)]
enum Scan {
    Full,
    Cache,
}

fn scan_command(root: PathBuf, revision: Revision, cause: Scan) -> LibraryCmd {
    match cause {
        Scan::Full => LibraryCmd::Rescan { root, revision },
        Scan::Cache => LibraryCmd::ScanLibrary { root, revision },
    }
}

struct LibraryLoop<'a> {
    watch: LibraryWatch,
    watcher: Option<notify::RecommendedWatcher>,
    filesystem_events: Receiver<notify::Result<notify::Event>>,
    cover_inbox: Receiver<CoverRequest>,
    decoding: Decoding,
    cover_cache: CoverCache,
    last_cover_side: Option<u32>,
    deadline: Option<Instant>,
    paths: LibraryPaths,
    decodable: &'static [&'static str],
    mailbox: &'a Mailbox<LibraryFact>,
    decoded: &'a Sender<CoverDecoded>,
}

impl<'a> LibraryLoop<'a> {
    fn new(
        paths: LibraryPaths,
        decodable: &'static [&'static str],
        outbound: Outbound<'a>,
    ) -> Self {
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        let watcher = notify::recommended_watcher(events).map_or_else(
            |error| {
                if let Delivery::Closed = outbound.mailbox.send(watch_failure(&error)) {
                }
                None
            },
            Some,
        );
        Self {
            watch: LibraryWatch::default(),
            watcher,
            filesystem_events,
            cover_inbox: outbound.cover_inbox,
            decoding: Decoding::default(),
            cover_cache: CoverCache::default(),
            last_cover_side: None,
            deadline: None,
            paths,
            decodable,
            mailbox: outbound.mailbox,
            decoded: outbound.decoded,
        }
    }

    fn run(mut self, inbox: &Receiver<LibraryCmd>) {
        loop {
            match self.wait(inbox) {
                Wake::Command(command) => self.command(command),
                Wake::FilesystemChange(event) => {
                    self.drive(
                        LibraryWatchMessage::FilesystemChange(event),
                        Scan::Full,
                    );
                }
                Wake::DebounceElapsed => {
                    self.deadline = None;
                    self.drive(LibraryWatchMessage::DebounceElapsed, Scan::Full);
                }
                Wake::Cover(request) => {
                    self.last_cover_side = Some(request.side);
                    self.drive_decode(DecodeMessage::Request(request));
                }
                Wake::CoverInboxLost => {
                    self.cover_inbox = crossbeam_channel::never();
                }
                Wake::FilesystemEventsLost => {
                    self.filesystem_events = crossbeam_channel::never();
                }
                Wake::Stopped => return,
            }
        }
    }

    fn wait(&self, inbox: &Receiver<LibraryCmd>) -> Wake {
        let mut select = Select::new();
        let command_index = select.recv(inbox);
        let filesystem_index = select.recv(&self.filesystem_events);
        let cover_index = select.recv(&self.cover_inbox);
        let selected = match self.deadline {
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
        if operation.index() == cover_index {
            return operation
                .recv(&self.cover_inbox)
                .map_or(Wake::CoverInboxLost, Wake::Cover);
        }
        Wake::Stopped
    }

    fn command(&mut self, command: LibraryCmd) {
        match command {
            LibraryCmd::Rescan { root, revision } => {
                self.drive(LibraryWatchMessage::Rescan { root, revision }, Scan::Full);
            }
            LibraryCmd::ScanLibrary { root, revision } => {
                self.drive(LibraryWatchMessage::Rescan { root, revision }, Scan::Cache);
            }
            LibraryCmd::PrefetchCover(path) => self.prefetch_cover(path),
            other @ (LibraryCmd::AppendHistory { .. }
            | LibraryCmd::SaveFavorites(_)
            | LibraryCmd::LoadFavorites
            | LibraryCmd::Trash(_)
            | LibraryCmd::LoadHistory { .. }
            | LibraryCmd::SavePlaylist { .. }
            | LibraryCmd::TagTracks { .. }) => self.execute(other),
        }
    }

    fn drive(&mut self, message: LibraryWatchMessage, cause: Scan) {
        let (watch, io) = step(std::mem::take(&mut self.watch), message);
        self.watch = watch;
        self.act(io, cause);
    }

    fn act(&mut self, io: WatchIo, cause: Scan) {
        match io {
            WatchIo::Nothing => {}
            WatchIo::Move { from, to, revision } => {
                let outcome = relocate(&mut self.watcher, &from, &to);
                self.report_watch(outcome);
                self.execute(scan_command(to, revision, cause));
            }
            WatchIo::ArmDebounce => self.deadline = Some(Instant::now() + DEBOUNCE),
            WatchIo::Rescan { root, revision } => {
                self.execute(scan_command(root, revision, cause));
            }
            WatchIo::RegisterAndRescan { root, revision } => {
                self.mount(&root);
                self.execute(scan_command(root, revision, cause));
            }
            WatchIo::Report(error) => self.report_watch(Err(error)),
        }
    }

    fn mount(&mut self, root: &Path) {
        let outcome = register(&mut self.watcher, root);
        self.report_watch(outcome);
    }

    fn report_watch(&self, outcome: Result<(), notify::Error>) {
        if let Err(error) = outcome
            && let Delivery::Closed = self.deliver(watch_failure(&error))
        {}
    }

    fn drive_decode(&mut self, message: DecodeMessage) {
        let (decoding, io) = decode_step(std::mem::take(&mut self.decoding), message);
        self.decoding = decoding;
        self.act_decode(io);
    }

    fn act_decode(&mut self, io: DecodeIo) {
        if let DecodeIo::Decode(request) = io {
            let path = request.path.clone();
            let decoded = self.resolve(request);
            let _ = self.decoded.send(decoded);
            self.drive_decode(DecodeMessage::Decoded(path));
        }
    }

    fn prefetch_cover(&mut self, path: PathBuf) {
        let Some(side) = self.last_cover_side else {
            return;
        };
        let _ = self.resolve(CoverRequest { path, side });
    }

    fn resolve(&mut self, request: CoverRequest) -> CoverDecoded {
        if let Some(outcome) = self.cover_cache.answer(&request) {
            return CoverDecoded {
                path: request.path,
                side: request.side,
                outcome,
            };
        }
        let decoded = decode(&request);
        self.cover_cache.remember(&decoded);
        decoded
    }

    fn execute(&self, command: LibraryCmd) {
        match execute(command, &self.paths, self.decodable) {
            Ok(Executed {
                fact: Some(fact), ..
            }) => if let Delivery::Closed = self.deliver(fact) {},
            Ok(Executed { fact: None, .. }) => {}
            Err(error) => {
                let failure: LibraryFailure = (&error).into();
                if let Delivery::Closed = self.deliver(LibraryFact::Failed(failure)) {}
            }
        }
    }

    fn deliver(&self, fact: LibraryFact) -> Delivery {
        self.mailbox.send(fact)
    }
}

fn step(watch: LibraryWatch, message: LibraryWatchMessage) -> (LibraryWatch, WatchIo) {
    match watch.transition(message) {
        Ok(pair) => pair,
        Err(rejected) => (rejected.state, WatchIo::default()),
    }
}

fn decode_step(decoding: Decoding, message: DecodeMessage) -> (Decoding, DecodeIo) {
    match decoding.transition(message) {
        Ok(pair) => pair,
        Err(rejected) => (rejected.state, DecodeIo::default()),
    }
}

fn watch_failure(error: &notify::Error) -> LibraryFact {
    let fault = match &error.kind {
        notify::ErrorKind::Io(source) => source.kind().into(),
        notify::ErrorKind::PathNotFound => IoFault::Missing,
        notify::ErrorKind::Generic(_)
        | notify::ErrorKind::WatchNotFound
        | notify::ErrorKind::InvalidConfig(_)
        | notify::ErrorKind::MaxFilesWatch => IoFault::Other,
    };
    let path = error.paths.first().map_or_else(PathBuf::new, Clone::clone);
    LibraryFact::Failed(LibraryFailure::File {
        subject: LibrarySubject::Watch,
        path,
        fault,
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossbeam_channel::Receiver;
    use kernel::{
        LibraryCmd,
        LibraryFact,
        LibraryFailure,
        LibrarySubject,
        Message,
        domain::{Driver, Revision},
    };
    use library::LibraryPaths;

    use crate::{
        driver::DriverThread,
        library::{
            cover::{CoverCache, CoverOutcome, CoverRequest, Decoding},
            driver::{LibraryLoop, Wake, spawn},
            watch::LibraryWatch,
        },
        mailbox::{Congestion, Mailbox},
    };

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
    ) -> (DriverThread<LibraryCmd>, Receiver<Message>) {
        let (mailbox, messages) = crossbeam_channel::unbounded();
        let (thread, _covers, _decoded) =
            spawn(paths(directory), DECODABLE, &mailbox).unwrap();
        (thread, messages)
    }

    fn stopped(thread: DriverThread<LibraryCmd>) {
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
            .send(LibraryCmd::ScanLibrary {
                root: directory.path().to_path_buf(),
                revision: Revision::default(),
            })
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
            .send(LibraryCmd::ScanLibrary {
                root: directory.path().join("missing"),
                revision: Revision::default(),
            })
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
        let (thread, covers, decoded_events) =
            spawn(paths(&directory), DECODABLE, &mailbox).unwrap();
        covers
            .send(CoverRequest {
                path: track.clone(),
                side: 64,
            })
            .unwrap();
        let decoded = decoded_events.recv_timeout(RECV_TIMEOUT).unwrap();
        assert_eq!(decoded.path, track);
        assert!(matches!(decoded.outcome, CoverOutcome::NoArt));
        assert!(
            decoded_events.recv_timeout(SETTLE_TIMEOUT).is_err(),
            "one request must answer with exactly one decoded cover"
        );
        stopped(thread);
    }

    #[test]
    #[ignore = "hardware: needs FSEvents; run with --include-ignored"]
    fn a_burst_of_fs_events_settles_into_one_rescan_with_the_last_scan_revision() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("one.mp3"), b"stub").unwrap();
        let (thread, messages) = spawned(&directory);
        let revision = Revision::default().next();
        thread
            .commands
            .send(LibraryCmd::Rescan {
                root: directory.path().to_path_buf(),
                revision,
            })
            .unwrap();
        let initial = messages
            .recv_timeout(Duration::from_secs(2))
            .ok()
            .and_then(scanned_track_count);
        assert_eq!(
            initial,
            Some(1),
            "the initial rescan must see the seed file"
        );
        std::thread::sleep(Duration::from_millis(100));
        for index in 0..3 {
            std::fs::write(directory.path().join(format!("burst{index}.mp3")), b"stub")
                .unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut counted = None;
        while Instant::now() < deadline {
            if let Ok(message) = messages.recv_timeout(Duration::from_millis(200))
                && let Some(count) = scanned_track_count(message)
            {
                counted = Some(count);
                break;
            }
        }
        assert_eq!(
            counted,
            Some(4),
            "the debounced rescan must see every file from the burst"
        );
        assert!(
            messages.recv_timeout(Duration::from_millis(500)).is_err(),
            "the burst must coalesce into a single rescan"
        );
        stopped(thread);
    }

    #[test]
    fn a_lost_filesystem_watcher_becomes_never_without_stopping_the_loop() {
        let directory = tempfile::tempdir().unwrap();
        let (mailbox, _messages) = crossbeam_channel::unbounded();
        let mailbox = Mailbox::new(mailbox, Congestion::default());
        let (decoded, _decoded_events) = crossbeam_channel::unbounded();
        let (events, filesystem_events) = crossbeam_channel::unbounded();
        drop(events);
        let mut driver_loop = LibraryLoop {
            watch: LibraryWatch::default(),
            watcher: None,
            filesystem_events,
            cover_inbox: crossbeam_channel::never(),
            decoding: Decoding::default(),
            cover_cache: CoverCache::default(),
            last_cover_side: None,
            deadline: None,
            paths: paths(&directory),
            decodable: DECODABLE,
            mailbox: &mailbox,
            decoded: &decoded,
        };
        let (commands, inbox) = crossbeam_channel::unbounded();
        assert!(matches!(
            driver_loop.wait(&inbox),
            Wake::FilesystemEventsLost
        ));
        driver_loop.filesystem_events = crossbeam_channel::never();
        commands.send(LibraryCmd::LoadFavorites).unwrap();
        assert!(matches!(driver_loop.wait(&inbox), Wake::Command(_)));
    }
}
