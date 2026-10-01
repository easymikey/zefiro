use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use kernel::{
    IoError,
    LibraryCmd,
    LibraryError,
    LibraryEvent,
    LibrarySubject,
    cmd::ScanMode,
    update::{Machine, Rejected},
};
use strum::IntoStaticStr;

use crate::library::{
    cover::{
        CoverCache,
        CoverDecoded,
        CoverRequest,
        DecodeFinished,
        DecodeMessage,
        Decoding,
        DecodingError,
    },
    watch::{LibraryWatch, WatchEffect, WatchError, WatchMessage},
};

const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Default, PartialEq)]
pub(crate) struct LibraryState {
    watch: Box<LibraryWatch>,
    decoding: Decoding,
    cover_cache: CoverCache,
    scan_mode: ScanMode,
    debounce: Option<Instant>,
}

#[derive(Debug, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum LibraryMessage {
    Cmd(LibraryCmd),
    FilesChanged {
        at: Instant,
        event: Result<(), notify::Error>,
    },
    EventsOverflowed {
        at: Instant,
    },
    DebounceDue,
    Cover(CoverRequest),
    Decoded(DecodeFinished),
    Executed(Result<Option<LibraryEvent>, library::Error>),
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryDriverError {
    Watch(WatchError),
    Decoding(DecodingError),
}

#[derive(Debug)]
pub(crate) enum WatchChange {
    Watch(PathBuf),
    Rewatch { from: PathBuf, to: PathBuf },
}

#[derive(Debug)]
pub(crate) enum LibraryEffect {
    Execute(LibraryCmd),
    Watch(WatchChange),
    Decode(CoverRequest),
    Publish(CoverDecoded),
    Event(LibraryEvent),
}

type Step = Result<(LibraryState, Vec<LibraryEffect>), Rejected<LibraryState>>;

impl Machine for LibraryState {
    type Message = LibraryMessage;
    type Error = LibraryDriverError;
    type Effect = Vec<LibraryEffect>;

    fn transition(self, input: LibraryMessage) -> Step {
        match input {
            LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir,
                revision,
                mode,
            }) => self.drive_scan(
                WatchMessage::Rescan {
                    music_dir,
                    revision,
                },
                mode,
            ),
            LibraryMessage::Cmd(other) => {
                Ok((self, vec![LibraryEffect::Execute(other)]))
            }
            LibraryMessage::FilesChanged { at, event } => {
                self.drive_filesystem_change(event, at)
            }
            LibraryMessage::EventsOverflowed { at } => {
                self.drive_filesystem_change(Ok(()), at)
            }
            LibraryMessage::DebounceDue => {
                let mut driven = self;
                driven.debounce = None;
                driven.drive_scan(WatchMessage::DebounceElapsed, ScanMode::Full)
            }
            LibraryMessage::Cover(request) => self.request_decode(request),
            LibraryMessage::Decoded(done) => Ok(self.finish_decode(done)),
            LibraryMessage::Executed(result) => Ok((self, executed_outputs(result))),
            LibraryMessage::Stopping => Ok((self, Vec::new())),
        }
    }
}

impl LibraryState {
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.debounce
    }

    fn drive_scan(mut self, message: WatchMessage, scan_mode: ScanMode) -> Step {
        self.scan_mode = scan_mode;
        let watch = std::mem::take(&mut self.watch);
        let result = watch.transition(message);
        self.apply_watch_result(result)
    }

    fn drive_filesystem_change(
        mut self,
        event: Result<(), notify::Error>,
        at: Instant,
    ) -> Step {
        self.scan_mode = ScanMode::Full;
        let watch = std::mem::take(&mut self.watch);
        let result = watch.transition(WatchMessage::FilesystemChange(event));
        if let Ok((_, WatchEffect::ArmDebounce)) = &result {
            self.debounce = Some(at + DEBOUNCE);
        }
        self.apply_watch_result(result)
    }

    fn apply_watch_result(
        mut self,
        result: Result<(LibraryWatch, WatchEffect), Rejected<LibraryWatch>>,
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
                    reason: LibraryDriverError::Watch(reason),
                })
            }
        }
    }

    fn outputs_for(&self, io: WatchEffect) -> Vec<LibraryEffect> {
        match io {
            WatchEffect::Nothing | WatchEffect::ArmDebounce => Vec::new(),
            WatchEffect::Rename { from, to, revision } => vec![
                LibraryEffect::Watch(WatchChange::Rewatch {
                    from,
                    to: to.clone(),
                }),
                LibraryEffect::Execute(LibraryCmd::Scan {
                    music_dir: to,
                    revision,
                    mode: self.scan_mode,
                }),
            ],
            WatchEffect::Rescan {
                music_dir,
                revision,
            } => {
                vec![LibraryEffect::Execute(LibraryCmd::Scan {
                    music_dir,
                    revision,
                    mode: self.scan_mode,
                })]
            }
            WatchEffect::RegisterAndRescan {
                music_dir,
                revision,
            } => vec![
                LibraryEffect::Watch(WatchChange::Watch(music_dir.clone())),
                LibraryEffect::Execute(LibraryCmd::Scan {
                    music_dir,
                    revision,
                    mode: self.scan_mode,
                }),
            ],
            WatchEffect::Report(error) => {
                vec![LibraryEffect::Event(watch_failure(&error))]
            }
        }
    }

    fn request_decode(mut self, request: CoverRequest) -> Step {
        if let Some(outcome) = self.cover_cache.answer(&request) {
            let decoded = CoverDecoded {
                path: request.path,
                side: request.size_px,
                outcome,
            };
            return Ok((self, vec![LibraryEffect::Publish(decoded)]));
        }
        let decoding = std::mem::take(&mut self.decoding);
        match decoding.transition(DecodeMessage::Request(request)) {
            Ok((decoding, io)) => {
                self.decoding = decoding;
                let outputs = io.map(LibraryEffect::Decode).into_iter().collect();
                Ok((self, outputs))
            }
            Err(Rejected { state, reason }) => {
                self.decoding = state;
                Err(Rejected {
                    state: self,
                    reason: LibraryDriverError::Decoding(reason),
                })
            }
        }
    }

    fn finish_decode(mut self, done: DecodeFinished) -> (Self, Vec<LibraryEffect>) {
        self.cover_cache.remember(&done);
        let decoding = std::mem::take(&mut self.decoding);
        self.decoding = match decoding.transition(DecodeMessage::Decoded(done.path)) {
            Ok((decoding, _)) => decoding,
            Err(Rejected { state, .. }) => state,
        };
        (self, Vec::new())
    }
}

fn executed_outputs(
    result: Result<Option<LibraryEvent>, library::Error>,
) -> Vec<LibraryEffect> {
    match result {
        Ok(event) => event.into_iter().map(LibraryEffect::Event).collect(),
        Err(error) => {
            let failure: LibraryError = (&error).into();
            vec![LibraryEffect::Event(LibraryEvent::Error(failure))]
        }
    }
}

pub(crate) fn watch_failure(error: &notify::Error) -> LibraryEvent {
    let kind = match &error.kind {
        notify::ErrorKind::Io(source) => source.kind().into(),
        notify::ErrorKind::PathNotFound => IoError::Missing,
        notify::ErrorKind::Generic(_)
        | notify::ErrorKind::WatchNotFound
        | notify::ErrorKind::InvalidConfig(_)
        | notify::ErrorKind::MaxFilesWatch => IoError::Other,
    };
    let path = error.paths.first().map_or_else(PathBuf::new, Clone::clone);
    LibraryEvent::Error(LibraryError::File {
        subject: LibrarySubject::Watch,
        path,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use kernel::{
        LibraryCmd,
        LibraryEvent,
        cmd::ScanMode,
        domain::Revision,
        update::{Machine, Rejected},
    };

    use crate::library::{
        cover::{
            CachedOutcome,
            CoverDecoded,
            CoverOutcome,
            CoverRequest,
            DecodeFinished,
            DecodingError,
        },
        machine::{
            DEBOUNCE,
            LibraryDriverError,
            LibraryEffect,
            LibraryMessage,
            LibraryState,
            WatchChange,
        },
    };

    #[test]
    fn a_rescan_command_executes_a_full_scan() {
        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default(),
                mode: ScanMode::Full,
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                LibraryEffect::Watch(WatchChange::Watch(_)),
                LibraryEffect::Execute(LibraryCmd::Scan {
                    mode: ScanMode::Full,
                    ..
                }),
            ]
        ));
    }

    #[test]
    fn scan_library_executes_a_cached_scan() {
        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default(),
                mode: ScanMode::Cached,
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [
                LibraryEffect::Watch(WatchChange::Watch(_)),
                LibraryEffect::Execute(LibraryCmd::Scan {
                    mode: ScanMode::Cached,
                    ..
                }),
            ]
        ));
    }

    #[test]
    fn a_file_change_arms_the_debounce_and_the_due_debounce_rescans_once() {
        let music_dir = PathBuf::from("/music");
        let (driver, _) = LibraryState::default()
            .transition(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: music_dir.clone(),
                revision: Revision::default(),
                mode: ScanMode::Full,
            }))
            .unwrap();

        let at = Instant::now();
        let (armed, outputs) = driver
            .transition(LibraryMessage::FilesChanged { at, event: Ok(()) })
            .unwrap();
        assert!(outputs.is_empty());
        assert_eq!(armed.deadline(), Some(at + DEBOUNCE));

        let (_, due_outputs) = armed.transition(LibraryMessage::DebounceDue).unwrap();
        assert!(matches!(
            due_outputs.as_slice(),
            [LibraryEffect::Execute(LibraryCmd::Scan { music_dir: rescanned, .. })]
                if *rescanned == music_dir
        ));
    }

    #[test]
    fn an_overflowed_event_channel_rescans_once() {
        let music_dir = PathBuf::from("/music");
        let (driver, _) = LibraryState::default()
            .transition(LibraryMessage::Cmd(LibraryCmd::Scan {
                music_dir: music_dir.clone(),
                revision: Revision::default(),
                mode: ScanMode::Full,
            }))
            .unwrap();

        let at = Instant::now();
        let (armed, outputs) = driver
            .transition(LibraryMessage::EventsOverflowed { at })
            .unwrap();
        assert!(outputs.is_empty());

        let (_, due_outputs) = armed.transition(LibraryMessage::DebounceDue).unwrap();
        assert!(matches!(
            due_outputs.as_slice(),
            [LibraryEffect::Execute(LibraryCmd::Scan { music_dir: rescanned, .. })]
                if *rescanned == music_dir
        ));
    }

    #[test]
    fn an_executed_fact_is_told() {
        let executed = Some(LibraryEvent::Loaded {
            tracks: Vec::new(),
            revision: Revision::default(),
        });

        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Executed(Ok(executed)))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryEffect::Event(LibraryEvent::Loaded { .. })]
        ));
    }

    #[test]
    fn an_execution_error_tells_a_library_failure() {
        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Executed(Err(library::Error::NoUserDirs)))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryEffect::Event(LibraryEvent::Error(_))]
        ));
    }

    #[test]
    fn an_execution_with_no_event_tells_nothing() {
        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Executed(Ok(None)))
            .unwrap();

        assert!(outputs.is_empty());
    }

    #[test]
    fn a_cover_request_while_idle_decodes() {
        let request = CoverRequest {
            path: PathBuf::from("/music/one.mp3"),
            size_px: 64,
        };

        let (_, outputs) = LibraryState::default()
            .transition(LibraryMessage::Cover(request))
            .unwrap();

        assert!(matches!(outputs.as_slice(), [LibraryEffect::Decode(_)]));
    }

    #[test]
    fn a_cover_request_while_decoding_waits() {
        let path = PathBuf::from("/music/one.mp3");
        let (driver, _) = LibraryState::default()
            .transition(LibraryMessage::Cover(CoverRequest {
                path: path.clone(),
                size_px: 64,
            }))
            .unwrap();

        let result = driver
            .transition(LibraryMessage::Cover(CoverRequest { path, size_px: 64 }));

        assert!(matches!(
            result,
            Err(Rejected {
                reason: LibraryDriverError::Decoding(DecodingError::WhileBusy),
                ..
            })
        ));
    }

    fn finished(path: &str) -> DecodeFinished {
        DecodeFinished {
            path: PathBuf::from(path),
            side: 64,
            cached: Some(CachedOutcome::NoArt),
        }
    }

    #[test]
    fn a_cached_cover_is_published_without_a_decode() {
        let mut driver = LibraryState::default();
        driver.cover_cache.remember(&finished("/music/one.mp3"));

        let (_, outputs) = driver
            .transition(LibraryMessage::Cover(CoverRequest {
                path: PathBuf::from("/music/one.mp3"),
                size_px: 64,
            }))
            .unwrap();

        assert!(matches!(
            outputs.as_slice(),
            [LibraryEffect::Publish(CoverDecoded {
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
            size_px: 64,
        };
        let (driver, first) = LibraryState::default()
            .transition(LibraryMessage::Cover(request()))
            .unwrap();
        let (driver, _) = driver
            .transition(LibraryMessage::Decoded(finished("/music/one.mp3")))
            .unwrap();

        let (_, second) = driver.transition(LibraryMessage::Cover(request())).unwrap();

        assert!(matches!(first.as_slice(), [LibraryEffect::Decode(_)]));
        assert!(matches!(second.as_slice(), [LibraryEffect::Publish(_)]));
    }
}
