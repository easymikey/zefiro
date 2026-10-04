use std::path::{Path, PathBuf};

use crossbeam_channel::{Receiver, RecvError, TrySendError};
use kernel::IoError;
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

fn watch_if_present(
    watcher: &mut impl Watcher,
    directory: &Path,
) -> Result<(), notify::Error> {
    if !directory.exists() {
        return Ok(());
    }
    watcher.watch(directory)
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

pub(crate) type Changed<M> = fn(&Path, Result<(), IoError>) -> M;

pub(crate) struct FileStream<M> {
    watcher: Option<RecommendedWatcher>,
    events: Receiver<notify::Result<notify::Event>>,
    item: Option<Changed<M>>,
    watched: PathBuf,
}

impl<M> FileStream<M> {
    pub(crate) fn idle() -> Self {
        Self {
            watcher: None,
            events: crossbeam_channel::never(),
            item: None,
            watched: PathBuf::new(),
        }
    }

    pub(crate) fn events(&self) -> &Receiver<notify::Result<notify::Event>> {
        &self.events
    }

    pub(crate) fn watch(&mut self, path: &Path, changed: Changed<M>) -> Option<M> {
        self.item = Some(changed);
        self.watched = path.to_path_buf();
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
                return Some(changed(path, Err(error)));
            }
        }
        watch_if_present(&mut self.watcher, path)
            .err()
            .map(|error| changed(path, Err(io_error(&error))))
    }

    pub(crate) fn unwatch(&mut self, path: &Path) -> Option<M> {
        let item = self.item?;
        self.watcher
            .unwatch(path)
            .err()
            .filter(|error| !matches!(error.kind, notify::ErrorKind::WatchNotFound))
            .map(|error| item(path, Err(io_error(&error))))
    }

    pub(crate) fn heard(
        &self,
        received: Result<notify::Result<notify::Event>, RecvError>,
    ) -> Option<M> {
        let event = received.ok()?;
        let item = self.item?;
        Some(item(
            &self.watched,
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

    use notify::RecommendedWatcher;

    use crate::watcher::{Watcher, watch_if_present};

    #[test]
    fn watching_without_a_watcher_reports_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let directory = std::env::temp_dir();

        let outcome = watch_if_present(&mut watcher, &directory);

        assert!(outcome.is_err());
    }

    #[test]
    fn watching_a_missing_directory_is_not_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let missing = std::env::temp_dir().join("does-not-exist-config-watcher-test");

        let outcome = watch_if_present(&mut watcher, &missing);

        assert!(outcome.is_ok());
    }

    #[test]
    fn unwatching_without_a_watcher_is_a_no_op() {
        let mut watcher: Option<RecommendedWatcher> = None;

        assert!(watcher.unwatch(Path::new("/music")).is_ok());
    }
}
