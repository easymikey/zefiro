use std::path::Path;

use audio::DECODABLE_EXTENSIONS;
use crossbeam_channel::Sender;
use kernel::{
    IoError,
    LibraryCmd,
    LibraryError,
    LibraryEvent,
    LibrarySubject,
    domain::DriverName,
};
use library::{LibraryDriver, LibraryEffect, LibraryJob, LibraryMessage};

use crate::{
    driver::{DriverLoop, DriverThread, LoopEffect, spawn_idle},
    error::Error,
    jobs::Jobs,
    registry,
    spawn::SpawnSetup,
};

pub(crate) fn idle_library(
    setup: &SpawnSetup<'_>,
) -> Result<(DriverThread<LibraryCmd>, Sender<LibraryMessage>), Error> {
    let thread = spawn_idle(registry::row(DriverName::Library), setup.inbox)?;
    let (covers, unheard) = crossbeam_channel::bounded(1);
    drop(unheard);
    Ok((thread, covers))
}

fn library_split(
    effect: LibraryEffect,
) -> LoopEffect<LibraryEffect, LibraryJob, LibraryMessage> {
    match effect {
        LibraryEffect::Run(job) => LoopEffect::Run(job),
        LibraryEffect::After { delay, timer } => LoopEffect::After {
            delay,
            message: LibraryMessage::Elapsed(timer),
        },
        LibraryEffect::Watch(path) => LoopEffect::Watch {
            path,
            item: library_watched,
        },
        LibraryEffect::Unwatch(path) => LoopEffect::Unwatch(path),
        effect @ (LibraryEffect::Publish(_) | LibraryEffect::Execute(_)) => {
            LoopEffect::Execute(effect)
        }
    }
}

fn library_watched(path: &Path, changed: Result<(), IoError>) -> LibraryMessage {
    match changed {
        Ok(()) => LibraryMessage::Changed,
        Err(kind) => LibraryMessage::Executed {
            event: LibraryEvent::Error(LibraryError::File {
                subject: LibrarySubject::Watch,
                path: path.to_path_buf(),
                kind,
            }),
            skipped: None,
        },
    }
}

pub(crate) fn spawn_library(
    setup: &SpawnSetup<'_>,
) -> Result<(DriverThread<LibraryCmd>, Sender<LibraryMessage>), Error> {
    let dirs = setup.paths.library.clone();
    let cover = setup.writers.cover.clone();
    let (covers, heard) = crossbeam_channel::bounded(64);
    let jobs = Jobs {
        split: library_split,
        run: LibraryJob::run,
    };
    let thread = DriverLoop::<LibraryDriver<_>, LibraryJob> {
        row: registry::row(DriverName::Library),
        inbox: setup.inbox.clone(),
        heard,
        seed: None,
        jobs,
    }
    .spawn(move || {
        LibraryDriver::new(dirs, DECODABLE_EXTENSIONS, move |decoded| {
            cover.publish(decoded);
        })
    })?;
    Ok((thread, covers))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        LibraryCmd,
        LibraryEvent,
        Message,
        Track,
        cmd::ScanMode,
        domain::{DriverName, Revision, Startup},
    };
    use library::{CoverArt, CoverDecoded, CoverJob, LibraryMessage};

    use crate::{
        driver::DriverThread,
        latest::LatestReceivers,
        spawn::{
            SpawnSetup,
            library_thread::spawn_library,
            tests::{RECV_TIMEOUT, stub_paths},
        },
    };

    const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    struct LibraryRun {
        thread: DriverThread<LibraryCmd>,
        covers: Sender<LibraryMessage>,
        messages: Receiver<Message>,
        cells: LatestReceivers,
        doorbell: Receiver<()>,
    }

    impl LibraryRun {
        fn start(directory: &Path) -> Self {
            let paths = stub_paths(directory);
            let (inbox, messages) = unbounded();
            let (model, _cmd) = kernel::startup(Startup::default());
            let (writers, cells, doorbell) = crate::latest::latest_channels();
            let (thread, covers) = spawn_library(&SpawnSetup {
                audio: &model.settings.audio,
                theme: &model.themes.selected,
                paths: &paths,
                inbox: &inbox,
                writers: &writers,
                #[cfg(target_os = "macos")]
                macos: &crate::macos::MacosChannel::new(),
            })
            .unwrap();
            Self {
                thread,
                covers,
                messages,
                cells,
                doorbell,
            }
        }

        fn scan(&self, music_dir: &Path) {
            self.thread
                .commands
                .send(LibraryCmd::Scan {
                    music_dir: music_dir.to_path_buf(),
                    revision: Revision::default(),
                    mode: ScanMode::Cached,
                })
                .unwrap();
        }

        fn ask(&self, path: &Path) {
            self.covers
                .send(LibraryMessage::Cover(CoverJob::new(
                    path.to_path_buf(),
                    kernel::domain::geometry::Pixels(64),
                )))
                .unwrap();
        }

        fn cover(&self, path: &Path) -> Arc<CoverDecoded> {
            self.ask(path);
            self.doorbell.recv_timeout(RECV_TIMEOUT).unwrap();
            self.cells.cover.take().unwrap()
        }

        fn stop(self) -> Receiver<Message> {
            drop(self.thread.commands);
            self.thread.handle.join().unwrap().unwrap();
            self.messages
        }
    }

    fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        collected.extend(std::iter::from_fn(|| {
            receiver.recv_timeout(SETTLE_TIMEOUT).ok()
        }));
        collected
    }

    fn listed_tracks(message: Message) -> Option<Vec<Arc<Track>>> {
        let Message::Library(LibraryEvent::Listed { tracks, .. }) = message else {
            return None;
        };
        Some(tracks)
    }

    #[test]
    fn a_scan_of_a_music_dir_lists_its_tracks() {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("one.mp3"), b"stub").unwrap();
        std::fs::write(music.join("two.flac"), b"stub").unwrap();
        let run = LibraryRun::start(directory.path());

        run.scan(&music);

        let listed = drain(&run.messages).into_iter().find_map(listed_tracks);
        assert_eq!(listed.map(|tracks| tracks.len()), Some(2));
        run.stop();
    }

    #[test]
    fn a_scan_of_a_missing_root_reports_a_library_error() {
        let directory = tempfile::tempdir().unwrap();
        let run = LibraryRun::start(directory.path());

        run.scan(&directory.path().join("missing"));

        let failed = drain(&run.messages)
            .into_iter()
            .any(|message| matches!(message, Message::Library(LibraryEvent::Error(_))));
        assert!(failed, "a missing music dir must report a library error");
        run.stop();
    }

    #[test]
    fn a_stopped_library_thread_ends_and_reports_its_stop() {
        let directory = tempfile::tempdir().unwrap();
        let run = LibraryRun::start(directory.path());

        let messages = run.stop();

        let reported = drain(&messages).into_iter().any(|message| {
            matches!(
                message,
                Message::Driver {
                    driver: DriverName::Library,
                    ..
                }
            )
        });
        assert!(
            reported,
            "the thread must report its stop through the inbox"
        );
    }

    #[test]
    fn a_cover_request_publishes_once_and_a_repeat_is_dropped() {
        let directory = tempfile::tempdir().unwrap();
        let track = directory.path().join("untagged.mp3");
        std::fs::write(&track, b"stub").unwrap();
        let run = LibraryRun::start(directory.path());

        let decoded = run.cover(&track);
        run.ask(&track);

        assert_eq!(decoded.path, track);
        assert!(matches!(decoded.art, CoverArt::Missing));
        assert!(
            run.doorbell.recv_timeout(SETTLE_TIMEOUT).is_err(),
            "a repeated request must not publish again"
        );
        run.stop();
    }

    #[test]
    fn a_return_to_an_earlier_cover_is_published_again() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mp3");
        let second = directory.path().join("second.mp3");
        std::fs::write(&first, b"stub").unwrap();
        std::fs::write(&second, b"stub").unwrap();
        let run = LibraryRun::start(directory.path());

        let paths = [&first, &second, &first].map(|path| run.cover(path).path.clone());

        assert_eq!(paths, [first.clone(), second, first]);
        run.stop();
    }
}
