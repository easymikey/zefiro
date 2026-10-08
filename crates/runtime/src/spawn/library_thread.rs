use audio::AUDIO_EXTENSIONS;
use kernel::{cmd::LibraryCmd, domain::driver::DriverName};
use library::{driver::LibraryDriver, job::LibraryJob};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::SpawnSetup,
};

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
    use std::{path::Path, sync::Arc};

    use crossbeam_channel::{Receiver, unbounded};
    use kernel::{
        cmd::{CoverJob, LibraryCmd, ScanMode},
        domain::{revision::Revision, track::Track},
        message::{LibraryEvent, Message},
    };
    use library::cover::{CoverDecoded, CoverLookup};
    use rstest::rstest;

    use crate::{
        driver_thread::DriverThread,
        latest::LatestReceivers,
        spawn::{
            library_thread::spawn_library,
            tests::{RECV_TIMEOUT, SETTLE_TIMEOUT, drain, spawned_with, stub_paths},
        },
    };

    struct LibraryRun {
        thread: DriverThread<LibraryCmd>,
        inbox_receiver: Receiver<Message>,
        latest_receivers: LatestReceivers,
        doorbell: Receiver<()>,
    }

    impl LibraryRun {
        fn start(dir: &Path) -> Self {
            let (inbox, inbox_receiver) = unbounded();
            let (thread, latest_receivers, doorbell) =
                spawned_with(spawn_library, &stub_paths(dir), &inbox);
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

        fn stop(self) {
            drop(self.thread.cmd_sender);
            self.thread.handle.join().unwrap();
        }
    }

    fn listed_tracks(message: Message) -> Option<Vec<Arc<Track>>> {
        let Message::Library(LibraryEvent::Listed { tracks, .. }) = message else {
            return None;
        };
        Some(tracks)
    }

    #[rstest]
    #[case::a_music_dir_lists_its_tracks(
        "music",
        &["one.mp3", "two.flac"],
        |message: Message| listed_tracks(message).is_some_and(|tracks| tracks.len() == 2)
    )]
    #[case::a_missing_root_reports_a_library_error(
        "missing",
        &[],
        |message: Message| matches!(message, Message::Library(LibraryEvent::Error(_)))
    )]
    fn a_scan_answers_through_the_inbox(
        #[case] music_dir: &str,
        #[case] files: &[&str],
        #[case] expected: fn(Message) -> bool,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let music_dir = directory.path().join(music_dir);
        for file in files {
            std::fs::create_dir_all(&music_dir).unwrap();
            std::fs::write(music_dir.join(file), b"stub").unwrap();
        }
        let run = LibraryRun::start(directory.path());

        run.scan(&music_dir);

        assert!(drain(&run.inbox_receiver).into_iter().any(expected));
        run.stop();
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
