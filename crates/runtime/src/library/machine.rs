use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use kernel::{
    IoFault,
    LibraryCmd,
    LibraryFact,
    LibraryFailure,
    LibrarySubject,
    domain::Revision,
    update::{Machine, Rejected},
};
use library::{Executed, LibraryError};
use strum::IntoStaticStr;

use crate::library::{
    cover::{
        CoverCache,
        CoverDecoded,
        CoverDone,
        CoverRequest,
        DecodeIo,
        DecodeMessage,
        Decoding,
        DecodingRejection,
    },
    watch::{LibraryWatch, LibraryWatchMessage, LibraryWatchRejection, WatchIo},
};

const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Scan {
    #[default]
    Full,
    Cache,
}

pub(crate) fn scan_command(
    root: PathBuf,
    revision: Revision,
    cause: Scan,
) -> LibraryCmd {
    match cause {
        Scan::Full => LibraryCmd::Rescan { root, revision },
        Scan::Cache => LibraryCmd::ScanLibrary { root, revision },
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Debounce {
    #[default]
    Idle,
    Until(Instant),
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct LibraryDriver {
    watch: Box<LibraryWatch>,
    decoding: Decoding,
    cover_cache: CoverCache,
    cause: Scan,
    debounce: Debounce,
}

#[derive(Debug, IntoStaticStr)]
pub(crate) enum LibraryInput {
    Command(LibraryCmd),
    FilesChanged {
        at: Instant,
        event: Result<(), notify::Error>,
    },
    EventsOverflowed {
        at: Instant,
    },
    DebounceDue,
    Cover(CoverRequest),
    Decoded(CoverDone),
    Executed(Result<Executed, LibraryError>),
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryDriverRejection {
    Watch(LibraryWatchRejection),
    Decoding(DecodingRejection),
}

#[derive(Debug)]
pub(crate) enum WatchChange {
    Register(PathBuf),
    Relocate { from: PathBuf, to: PathBuf },
}

#[derive(Debug)]
pub(crate) enum LibraryOutput {
    Execute(LibraryCmd),
    Watch(WatchChange),
    Decode(CoverRequest),
    Publish(CoverDecoded),
    Tell(LibraryFact),
}

type Step = Result<(LibraryDriver, Vec<LibraryOutput>), Rejected<LibraryDriver>>;

impl Machine for LibraryDriver {
    type Message = LibraryInput;
    type Rejection = LibraryDriverRejection;
    type Effect = Vec<LibraryOutput>;

    fn transition(self, input: LibraryInput) -> Step {
        match input {
            LibraryInput::Command(LibraryCmd::Rescan { root, revision }) => self
                .drive_scan(LibraryWatchMessage::Rescan { root, revision }, Scan::Full),
            LibraryInput::Command(LibraryCmd::ScanLibrary { root, revision }) => self
                .drive_scan(
                    LibraryWatchMessage::Rescan { root, revision },
                    Scan::Cache,
                ),
            LibraryInput::Command(other) => {
                Ok((self, vec![LibraryOutput::Execute(other)]))
            }
            LibraryInput::FilesChanged { at, event } => {
                self.drive_filesystem_change(event, at)
            }
            LibraryInput::EventsOverflowed { at } => {
                self.drive_filesystem_change(Ok(()), at)
            }
            LibraryInput::DebounceDue => {
                let mut driven = self;
                driven.debounce = Debounce::Idle;
                driven.drive_scan(LibraryWatchMessage::DebounceElapsed, Scan::Full)
            }
            LibraryInput::Cover(request) => self.request_decode(request),
            LibraryInput::Decoded(done) => Ok(self.finish_decode(done)),
            LibraryInput::Executed(result) => Ok((self, executed_outputs(result))),
            LibraryInput::Stopping => Ok((self, Vec::new())),
        }
    }
}

impl LibraryDriver {
    pub(crate) fn deadline(&self) -> Option<Instant> {
        match self.debounce {
            Debounce::Idle => None,
            Debounce::Until(at) => Some(at),
        }
    }

    fn drive_scan(mut self, message: LibraryWatchMessage, cause: Scan) -> Step {
        self.cause = cause;
        let watch = std::mem::take(&mut self.watch);
        let result = watch.transition(message);
        self.settle_watch(result)
    }

    fn drive_filesystem_change(
        mut self,
        event: Result<(), notify::Error>,
        at: Instant,
    ) -> Step {
        self.cause = Scan::Full;
        let watch = std::mem::take(&mut self.watch);
        let result = watch.transition(LibraryWatchMessage::FilesystemChange(event));
        if let Ok((_, WatchIo::ArmDebounce)) = &result {
            self.debounce = Debounce::Until(at + DEBOUNCE);
        }
        self.settle_watch(result)
    }

    fn settle_watch(
        mut self,
        result: Result<(LibraryWatch, WatchIo), Rejected<LibraryWatch>>,
    ) -> Step {
        match result {
            Ok((watch, io)) => {
                self.watch = Box::new(watch);
                let outputs = self.outputs_for(io);
                Ok((self, outputs))
            }
            Err(Rejected { state, reason }) => {
                self.watch = Box::new(state);
                Err(Rejected {
                    state: self,
                    reason: LibraryDriverRejection::Watch(reason),
                })
            }
        }
    }

    fn outputs_for(&self, io: WatchIo) -> Vec<LibraryOutput> {
        match io {
            WatchIo::Nothing | WatchIo::ArmDebounce => Vec::new(),
            WatchIo::Move { from, to, revision } => vec![
                LibraryOutput::Watch(WatchChange::Relocate {
                    from,
                    to: to.clone(),
                }),
                LibraryOutput::Execute(scan_command(to, revision, self.cause)),
            ],
            WatchIo::Rescan { root, revision } => {
                vec![LibraryOutput::Execute(scan_command(
                    root, revision, self.cause,
                ))]
            }
            WatchIo::RegisterAndRescan { root, revision } => vec![
                LibraryOutput::Watch(WatchChange::Register(root.clone())),
                LibraryOutput::Execute(scan_command(root, revision, self.cause)),
            ],
            WatchIo::Report(error) => vec![LibraryOutput::Tell(watch_failure(&error))],
        }
    }

    fn request_decode(mut self, request: CoverRequest) -> Step {
        if let Some(outcome) = self.cover_cache.answer(&request) {
            let decoded = CoverDecoded {
                path: request.path,
                side: request.side,
                outcome,
            };
            return Ok((self, vec![LibraryOutput::Publish(decoded)]));
        }
        let decoding = std::mem::take(&mut self.decoding);
        match decoding.transition(DecodeMessage::Request(request)) {
            Ok((decoding, io)) => {
                self.decoding = decoding;
                let outputs = match io {
                    DecodeIo::Decode(pending) => vec![LibraryOutput::Decode(pending)],
                    DecodeIo::Nothing => Vec::new(),
                };
                Ok((self, outputs))
            }
            Err(Rejected { state, reason }) => {
                self.decoding = state;
                Err(Rejected {
                    state: self,
                    reason: LibraryDriverRejection::Decoding(reason),
                })
            }
        }
    }

    fn finish_decode(mut self, done: CoverDone) -> (Self, Vec<LibraryOutput>) {
        self.cover_cache.remember(&done);
        let decoding = std::mem::take(&mut self.decoding);
        self.decoding = match decoding.transition(DecodeMessage::Decoded(done.path)) {
            Ok((decoding, _)) => decoding,
            Err(Rejected { state, .. }) => state,
        };
        (self, Vec::new())
    }
}

fn executed_outputs(result: Result<Executed, LibraryError>) -> Vec<LibraryOutput> {
    match result {
        Ok(Executed { fact, .. }) => {
            fact.into_iter().map(LibraryOutput::Tell).collect()
        }
        Err(error) => {
            let failure: LibraryFailure = (&error).into();
            vec![LibraryOutput::Tell(LibraryFact::Failed(failure))]
        }
    }
}

pub(crate) fn watch_failure(error: &notify::Error) -> LibraryFact {
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
    use std::{path::PathBuf, time::Instant};

    use kernel::{
        LibraryCmd,
        LibraryFact,
        domain::Revision,
        update::{Machine, Rejected},
    };
    use library::{Executed, LibraryError, LibraryNote};

    use crate::library::{
        cover::{
            CachedOutcome,
            CoverDecoded,
            CoverDone,
            CoverOutcome,
            CoverRequest,
            DecodingRejection,
        },
        machine::{
            DEBOUNCE,
            LibraryDriver,
            LibraryDriverRejection,
            LibraryInput,
            LibraryOutput,
            WatchChange,
        },
    };

    #[test]
    fn a_rescan_command_executes_a_full_scan() {
        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Command(LibraryCmd::Rescan {
                root: PathBuf::from("/music"),
                revision: Revision::default(),
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                LibraryOutput::Watch(WatchChange::Register(_)),
                LibraryOutput::Execute(LibraryCmd::Rescan { .. }),
            ]
        ));
    }

    #[test]
    fn scan_library_executes_a_cached_scan() {
        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Command(LibraryCmd::ScanLibrary {
                root: PathBuf::from("/music"),
                revision: Revision::default(),
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                LibraryOutput::Watch(WatchChange::Register(_)),
                LibraryOutput::Execute(LibraryCmd::ScanLibrary { .. }),
            ]
        ));
    }

    #[test]
    fn a_file_change_arms_the_debounce_and_the_due_debounce_rescans_once() {
        let root = PathBuf::from("/music");
        let (driver, _) = LibraryDriver::default()
            .transition(LibraryInput::Command(LibraryCmd::Rescan {
                root: root.clone(),
                revision: Revision::default(),
            }))
            .unwrap();

        let at = Instant::now();
        let (armed, outputs) = driver
            .transition(LibraryInput::FilesChanged { at, event: Ok(()) })
            .unwrap();
        assert!(outputs.is_empty());
        assert_eq!(armed.deadline(), Some(at + DEBOUNCE));

        let (_, due_outputs) = armed.transition(LibraryInput::DebounceDue).unwrap();
        assert!(matches!(
            due_outputs.as_slice(),
            [LibraryOutput::Execute(LibraryCmd::Rescan { root: rescanned, .. })]
                if *rescanned == root
        ));
    }

    #[test]
    fn an_overflowed_event_channel_rescans_once() {
        let root = PathBuf::from("/music");
        let (driver, _) = LibraryDriver::default()
            .transition(LibraryInput::Command(LibraryCmd::Rescan {
                root: root.clone(),
                revision: Revision::default(),
            }))
            .unwrap();

        let at = Instant::now();
        let (armed, outputs) = driver
            .transition(LibraryInput::EventsOverflowed { at })
            .unwrap();
        assert!(outputs.is_empty());

        let (_, due_outputs) = armed.transition(LibraryInput::DebounceDue).unwrap();
        assert!(matches!(
            due_outputs.as_slice(),
            [LibraryOutput::Execute(LibraryCmd::Rescan { root: rescanned, .. })]
                if *rescanned == root
        ));
    }

    #[test]
    fn an_executed_fact_is_told() {
        let executed = Executed {
            fact: Some(LibraryFact::Loaded {
                tracks: Vec::new(),
                revision: Revision::default(),
            }),
            notes: Vec::new(),
        };

        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Executed(Ok(executed)))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryOutput::Tell(LibraryFact::Loaded { .. })]
        ));
    }

    #[test]
    fn an_execution_error_tells_a_library_failure() {
        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Executed(Err(LibraryError::NoDirectory)))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryOutput::Tell(LibraryFact::Failed(_))]
        ));
    }

    #[test]
    fn notes_are_dropped() {
        let executed = Executed {
            fact: None,
            notes: vec![LibraryNote::HistoryLinesSkipped { lines: 3 }],
        };

        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Executed(Ok(executed)))
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn a_cover_request_while_idle_decodes() {
        let request = CoverRequest {
            path: PathBuf::from("/music/one.mp3"),
            side: 64,
        };

        let (_, outputs) = LibraryDriver::default()
            .transition(LibraryInput::Cover(request))
            .unwrap();

        assert!(matches!(outputs.as_slice(), [LibraryOutput::Decode(_)]));
    }

    #[test]
    fn a_cover_request_while_decoding_waits() {
        let path = PathBuf::from("/music/one.mp3");
        let (driver, _) = LibraryDriver::default()
            .transition(LibraryInput::Cover(CoverRequest {
                path: path.clone(),
                side: 64,
            }))
            .unwrap();

        let result =
            driver.transition(LibraryInput::Cover(CoverRequest { path, side: 64 }));

        assert!(matches!(
            result,
            Err(Rejected {
                reason: LibraryDriverRejection::Decoding(DecodingRejection::WhileBusy),
                ..
            })
        ));
    }

    fn finished(path: &str) -> CoverDone {
        CoverDone {
            path: PathBuf::from(path),
            side: 64,
            cached: Some(CachedOutcome::NoArt),
        }
    }

    #[test]
    fn a_cached_cover_is_published_without_a_decode() {
        let mut driver = LibraryDriver::default();
        driver.cover_cache.remember(&finished("/music/one.mp3"));

        let (_, outputs) = driver
            .transition(LibraryInput::Cover(CoverRequest {
                path: PathBuf::from("/music/one.mp3"),
                side: 64,
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryOutput::Publish(CoverDecoded {
                side: 64,
                outcome: CoverOutcome::NoArt,
                ..
            })]
        ));
    }

    #[test]
    fn a_finished_decode_is_remembered_for_the_next_request() {
        let request = || CoverRequest {
            path: PathBuf::from("/music/one.mp3"),
            side: 64,
        };
        let (driver, first) = LibraryDriver::default()
            .transition(LibraryInput::Cover(request()))
            .unwrap();
        let (driver, _) = driver
            .transition(LibraryInput::Decoded(finished("/music/one.mp3")))
            .unwrap();

        let (_, second) = driver.transition(LibraryInput::Cover(request())).unwrap();

        assert!(matches!(first.as_slice(), [LibraryOutput::Decode(_)]));
        assert!(matches!(second.as_slice(), [LibraryOutput::Publish(_)]));
    }
}
