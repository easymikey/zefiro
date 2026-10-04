use std::path::Path;

use crossbeam_channel::{Receiver, RecvError, TrySendError};
use kernel::domain::io_error::IoError;
use notify::{RecommendedWatcher, RecursiveMode};

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

pub(crate) trait Watcher {
    fn watch(&mut self, path: &Path) -> Result<(), notify::Error>;
    fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error>;
}

impl Watcher for RecommendedWatcher {
    fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
        notify::Watcher::watch(self, path, RecursiveMode::Recursive)
    }

    fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
        notify::Watcher::unwatch(self, path)
    }
}

impl<W: Watcher> Watcher for Option<W> {
    fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self.as_mut().map_or_else(
            || Err(notify::Error::generic("no watcher")),
            |watcher| watcher.watch(path),
        )
    }

    fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self.as_mut()
            .map_or(Ok(()), |watcher| watcher.unwatch(path))
    }
}

fn recommended<H: notify::EventHandler>(
    handler: H,
    report: impl FnOnce(&notify::Error),
) -> Option<RecommendedWatcher> {
    match notify::recommended_watcher(handler) {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            report(&error);
            None
        }
    }
}

pub(crate) type Changed<M> = fn(Result<(), IoError>) -> M;

pub(crate) struct FileStream<M> {
    watcher: Option<RecommendedWatcher>,
    events: Receiver<notify::Result<notify::Event>>,
    item: Option<Changed<M>>,
}

impl<M> FileStream<M> {
    pub(crate) fn idle() -> Self {
        Self {
            watcher: None,
            events: crossbeam_channel::never(),
            item: None,
        }
    }

    pub(crate) fn events(&self) -> &Receiver<notify::Result<notify::Event>> {
        &self.events
    }

    pub(crate) fn watch(&mut self, path: &Path, changed: Changed<M>) -> Option<M> {
        self.item = Some(changed);
        if self.watcher.is_none() {
            let (sender, events) = crossbeam_channel::bounded(1);
            let mut failure = None;
            self.watcher = recommended(
                move |event| match sender.try_send(event) {
                    Ok(())
                    | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
                },
                |error| failure = Some(io_error(error)),
            );
            self.events = events;
            if let Some(error) = failure {
                return Some(changed(Err(error)));
            }
        }
        if !path.exists() {
            return Some(changed(Err(IoError::Missing)));
        }
        self.watcher
            .watch(path)
            .err()
            .map(|error| changed(Err(io_error(&error))))
    }

    pub(crate) fn unwatch(&mut self, path: &Path) -> Option<M> {
        let item = self.item?;
        self.watcher
            .unwatch(path)
            .err()
            .filter(|error| !matches!(error.kind, notify::ErrorKind::WatchNotFound))
            .map(|error| item(Err(io_error(&error))))
    }

    pub(crate) fn heard(
        &self,
        received: Result<notify::Result<notify::Event>, RecvError>,
    ) -> Option<M> {
        let event = received.ok()?;
        let item = self.item?;
        Some(item(
            event.map(|_event| ()).map_err(|error| io_error(&error)),
        ))
    }

    pub(crate) fn lose(&mut self) {
        self.events = crossbeam_channel::never();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kernel::domain::io_error::IoError;
    use notify::RecommendedWatcher;

    use crate::watcher::{FileStream, Watcher};

    fn as_outcome(changed: Result<(), IoError>) -> Result<(), IoError> {
        changed
    }

    #[test]
    fn watching_without_a_watcher_reports_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let directory = std::env::temp_dir();

        let outcome = watcher.watch(&directory);

        assert!(outcome.is_err());
    }

    #[test]
    fn watching_a_missing_directory_reports_it_missing() {
        let mut files = FileStream::idle();
        let missing = std::env::temp_dir().join("does-not-exist-config-watcher-test");

        let outcome = files.watch(&missing, as_outcome);

        assert_eq!(outcome, Some(Err(IoError::Missing)));
    }

    #[test]
    fn unwatching_without_a_watcher_is_a_no_op() {
        let mut watcher: Option<RecommendedWatcher> = None;

        assert!(watcher.unwatch(Path::new("/music")).is_ok());
    }
}
