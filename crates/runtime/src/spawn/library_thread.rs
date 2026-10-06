use audio::AUDIO_EXTENSIONS;
use kernel::{cmd::LibraryCmd, domain::driver::DriverName};
use library::{driver::LibraryDriver, job::LibraryJob};

#[cfg(test)] use crate::driver_thread::spawn_idle;
use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::SpawnSetup,
};

#[cfg(test)]
pub(crate) fn idle_library(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<LibraryCmd>, SpawnError> {
    spawn_idle(registry::row(DriverName::Library), setup.inbox)
}

pub(crate) fn spawn_library(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<LibraryCmd>, SpawnError> {
    let library_dirs = setup.paths.library_dirs.clone();
    let cover_sender = setup.latest_senders.cover_sender.clone();
    let run_job = LibraryJob::run;
    DriverLoop::<LibraryDriver<_>, LibraryJob> {
        row: registry::row(DriverName::Library),
        inbox: setup.inbox.clone(),
        callback_receiver: crossbeam_channel::never(),
        message: None,
        run_job,
    }
    .spawn(move || {
        LibraryDriver::new(library_dirs, AUDIO_EXTENSIONS, move |cover_decoded| {
            cover_sender.publish(cover_decoded);
        })
    })
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use crossbeam_channel::{Receiver, unbounded};
    use kernel::{
        cmd::{CoverJob, LibraryCmd, ScanMode},
        domain::{
            driver::DriverName,
            revision::Revision,
            startup::Startup,
            track::Track,
        },
        message::{LibraryEvent, Message},
    };
    use library::cover::{CoverDecoded, CoverLookup};

    use crate::{
        driver_thread::DriverThread,
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
        inbox_receiver: Receiver<Message>,
        latest_receivers: LatestReceivers,
        doorbell: Receiver<()>,
    }

    impl LibraryRun {
        fn start(dir: &Path) -> Self {
            let paths = stub_paths(dir);
            let (inbox, inbox_receiver) = unbounded();
            let (model, _cmd) = kernel::update::startup::startup(Startup::default());
            let (latest_senders, latest_receivers, doorbell) =
                crate::latest::latest_channels();
            let thread = spawn_library(&SpawnSetup {
                audio_settings: &model.settings.audio_settings,
                paths: &paths,
                inbox: &inbox,
                latest_senders: &latest_senders,
                #[cfg(target_os = "macos")]
                macos_channel: &crate::spawn_setup::MacosChannel::new(),
            })
            .unwrap();
            Self {
                thread,
                inbox_receiver,
                latest_receivers,
                doorbell,
            }
        }

        fn scan(&self, music_dir: &Path) {
            self.thread
                .cmd_sender
                .send(LibraryCmd::Scan {
                    music_dir: music_dir.to_path_buf(),
                    revision: Revision::default(),
                    mode: ScanMode::Cached,
                })
                .unwrap();
        }

        fn decode_cover(&self, path: &Path) {
            self.thread
                .cmd_sender
                .send(LibraryCmd::DecodeCover(CoverJob {
                    path: path.to_path_buf(),
                    side: kernel::domain::geometry::Pixels(64),
                }))
                .unwrap();
        }

        fn cover(&self, path: &Path) -> Arc<CoverDecoded> {
            self.decode_cover(path);
            self.doorbell.recv_timeout(RECV_TIMEOUT).unwrap();
            self.latest_receivers.cover_receiver.take().unwrap()
        }

        fn stop(self) -> Receiver<Message> {
            drop(self.thread.cmd_sender);
            self.thread.handle.join().unwrap().unwrap();
            self.inbox_receiver
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
        let music_dir = directory.path().join("music");
        std::fs::create_dir_all(&music_dir).unwrap();
        std::fs::write(music_dir.join("one.mp3"), b"stub").unwrap();
        std::fs::write(music_dir.join("two.flac"), b"stub").unwrap();
        let run = LibraryRun::start(directory.path());

        run.scan(&music_dir);

        let listed = drain(&run.inbox_receiver)
            .into_iter()
            .find_map(listed_tracks);
        assert_eq!(listed.map(|tracks| tracks.len()), Some(2));
        run.stop();
    }

    #[test]
    fn a_scan_of_a_missing_root_reports_a_library_error() {
        let directory = tempfile::tempdir().unwrap();
        let run = LibraryRun::start(directory.path());

        run.scan(&directory.path().join("missing"));

        let has_library_error = drain(&run.inbox_receiver)
            .into_iter()
            .any(|message| matches!(message, Message::Library(LibraryEvent::Error(_))));
        assert!(
            has_library_error,
            "a missing music dir must report a library error"
        );
        run.stop();
    }

    #[test]
    fn a_stopped_library_thread_ends_and_reports_its_stop() {
        let directory = tempfile::tempdir().unwrap();
        let run = LibraryRun::start(directory.path());

        let inbox_receiver = run.stop();

        let reported = drain(&inbox_receiver).into_iter().any(|message| {
            matches!(
                message,
                Message::Driver {
                    driver_name: DriverName::Library,
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
        let path = directory.path().join("untagged.mp3");
        std::fs::write(&path, b"stub").unwrap();
        let run = LibraryRun::start(directory.path());

        let decoded = run.cover(&path);
        run.decode_cover(&path);

        assert_eq!(decoded.path, path);
        assert!(matches!(decoded.cover_lookup, CoverLookup::Missing));
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
