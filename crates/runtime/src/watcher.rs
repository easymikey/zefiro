use std::{mem, path::Path};

use crossbeam_channel::{Receiver, RecvError, TrySendError};
use kernel::domain::io_error::IoError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

fn io_error(error: &notify::Error) -> IoError {
    match &error.kind {
        notify::ErrorKind::Io(source) => source.kind().into(),
        notify::ErrorKind::PathNotFound => IoError::Missing,
        notify::ErrorKind::Generic(_)
        | notify::ErrorKind::WatchNotFound
        | notify::ErrorKind::InvalidConfig(_)
        | notify::ErrorKind::MaxFilesWatch => IoError::Other,
    }
}

pub(crate) type Changed<M> = fn(Result<(), IoError>) -> M;

type Events = Receiver<notify::Result<notify::Event>>;

pub(crate) enum FileStream<M> {
    Idle,
    Watching {
        watcher: RecommendedWatcher,
        events: Events,
        changed: Changed<M>,
    },
    Lost(Changed<M>),
}

fn started() -> Result<(RecommendedWatcher, Events), IoError> {
    let (sender, events) = crossbeam_channel::bounded(1);
    notify::recommended_watcher(move |event| match sender.try_send(event) {
        Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
    })
    .map(|watcher| (watcher, events))
    .map_err(|error| io_error(&error))
}

impl<M> FileStream<M> {
    pub(crate) fn events(&self) -> Option<&Events> {
        match self {
            Self::Watching { events, .. } => Some(events),
            Self::Idle | Self::Lost(_) => None,
        }
    }

    pub(crate) fn watch(&mut self, path: &Path, changed: Changed<M>) -> Option<M> {
        let (mut watcher, events) = match mem::replace(self, Self::Idle) {
            Self::Watching {
                watcher, events, ..
            } => (watcher, events),
            Self::Idle | Self::Lost(_) => match started() {
                Ok(started) => started,
                Err(error) => {
                    *self = Self::Lost(changed);
                    return Some(changed(Err(error)));
                }
            },
        };
        let failure = if path.exists() {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .err()
                .map(|error| io_error(&error))
        } else {
            Some(IoError::Missing)
        };
        *self = Self::Watching {
            watcher,
            events,
            changed,
        };
        failure.map(|error| changed(Err(error)))
    }

    pub(crate) fn unwatch(&mut self, path: &Path) -> Option<M> {
        let Self::Watching {
            watcher, changed, ..
        } = self
        else {
            return None;
        };
        watcher
            .unwatch(path)
            .err()
            .filter(|error| !matches!(error.kind, notify::ErrorKind::WatchNotFound))
            .map(|error| changed(Err(io_error(&error))))
    }

    pub(crate) fn heard(
        &self,
        received: Result<notify::Result<notify::Event>, RecvError>,
    ) -> Option<M> {
        let (Self::Watching { changed, .. } | Self::Lost(changed)) = self else {
            return None;
        };
        let event = received.ok()?;
        Some(changed(
            event.map(|_event| ()).map_err(|error| io_error(&error)),
        ))
    }

    pub(crate) fn lose(&mut self) -> Option<M> {
        let Self::Watching { changed, .. } = self else {
            return None;
        };
        let changed = *changed;
        *self = Self::Lost(changed);
        Some(changed(Err(IoError::Other)))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use crossbeam_channel::RecvError;
    use kernel::domain::io_error::IoError;
    use rstest::rstest;

    use crate::watcher::{FileStream, io_error};

    fn as_outcome(changed: Result<(), IoError>) -> Result<(), IoError> {
        changed
    }

    #[test]
    fn watching_a_missing_directory_reports_it_missing() {
        let mut file_stream = FileStream::Idle;
        let missing = std::env::temp_dir().join("does-not-exist-config-watcher-test");

        let outcome = file_stream.watch(&missing, as_outcome);

        assert_eq!(outcome, Some(Err(IoError::Missing)));
    }

    #[rstest]
    #[case::io_denied(
        notify::Error::io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        IoError::Denied
    )]
    #[case::path_not_found(notify::Error::path_not_found(), IoError::Missing)]
    #[case::generic(notify::Error::generic("broken"), IoError::Other)]
    #[case::watch_not_found(notify::Error::watch_not_found(), IoError::Other)]
    #[case::max_files_watch(
        notify::Error::new(notify::ErrorKind::MaxFilesWatch),
        IoError::Other
    )]
    fn a_notify_error_maps_to_its_io_error(
        #[case] error: notify::Error,
        #[case] expected: IoError,
    ) {
        assert_eq!(io_error(&error), expected);
    }

    #[rstest]
    #[case::an_event(Ok(Ok(notify::Event::default())), Some(Ok(())))]
    #[case::a_failed_event(
        Ok(Err(notify::Error::path_not_found())),
        Some(Err(IoError::Missing))
    )]
    #[case::a_closed_channel(Err(RecvError), None)]
    fn a_watched_stream_turns_what_it_hears_into_its_message(
        #[case] received: Result<notify::Result<notify::Event>, RecvError>,
        #[case] expected: Option<Result<(), IoError>>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let mut file_stream = FileStream::Idle;
        assert_eq!(file_stream.watch(directory.path(), as_outcome), None);

        assert_eq!(file_stream.heard(received), expected);
    }

    #[test]
    fn an_idle_stream_hears_nothing() {
        let file_stream: FileStream<Result<(), IoError>> = FileStream::Idle;

        assert_eq!(file_stream.heard(Ok(Ok(notify::Event::default()))), None);
    }

    #[test]
    #[ignore = "hardware: FSEvents"]
    fn a_changed_file_in_a_watched_directory_produces_its_message() {
        let directory = tempfile::tempdir().unwrap();
        let mut file_stream = FileStream::Idle;
        assert_eq!(file_stream.watch(directory.path(), as_outcome), None);

        std::fs::write(directory.path().join("config.toml"), "volume = 1").unwrap();
        let received = file_stream
            .events()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_timeout| RecvError);

        assert_eq!(file_stream.heard(received), Some(Ok(())));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Stage {
        Idle,
        Watching,
        Lost,
    }

    fn stage<M>(file_stream: &FileStream<M>) -> Stage {
        match file_stream {
            FileStream::Idle => Stage::Idle,
            FileStream::Watching { .. } => Stage::Watching,
            FileStream::Lost(_) => Stage::Lost,
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Step {
        Watch,
        WatchMissing,
        Lose,
    }

    #[rstest]
    #[case::idle_stays_idle_when_lost(&[Step::Lose], Stage::Idle)]
    #[case::idle_watches(&[Step::Watch], Stage::Watching)]
    #[case::a_missing_directory_is_still_watched(&[Step::WatchMissing], Stage::Watching)]
    #[case::watching_loses_its_events(&[Step::Watch, Step::Lose], Stage::Lost)]
    #[case::lost_watches_again(&[Step::Watch, Step::Lose, Step::Watch], Stage::Watching)]
    #[case::lost_stays_lost_when_lost_again(
        &[Step::Watch, Step::Lose, Step::Lose],
        Stage::Lost
    )]
    fn a_file_stream_goes_from_idle_to_watching_to_lost(
        #[case] steps: &[Step],
        #[case] expected: Stage,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");
        let mut file_stream = FileStream::Idle;

        for step in steps {
            match step {
                Step::Watch => {
                    assert_eq!(file_stream.watch(directory.path(), as_outcome), None);
                }
                Step::WatchMissing => {
                    assert_eq!(
                        file_stream.watch(&missing, as_outcome),
                        Some(Err(IoError::Missing))
                    );
                }
                Step::Lose => {
                    file_stream.lose();
                }
            }
        }

        assert_eq!(stage(&file_stream), expected);
    }

    #[test]
    fn a_lost_watch_is_reported_as_an_unknown_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut file_stream = FileStream::Idle;
        assert_eq!(file_stream.watch(directory.path(), as_outcome), None);

        assert_eq!(file_stream.lose(), Some(Err(IoError::Other)));
        assert_eq!(file_stream.lose(), None);
    }

    #[test]
    fn a_lost_stream_still_hears_but_watches_no_directory() {
        let mut file_stream: FileStream<Result<(), IoError>> =
            FileStream::Lost(as_outcome);

        assert_eq!(
            file_stream.heard(Ok(Ok(notify::Event::default()))),
            Some(Ok(()))
        );
        assert_eq!(file_stream.unwatch(Path::new("/music")), None);
        assert!(file_stream.events().is_none());
    }
}
