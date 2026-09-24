use std::path::Path;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub(crate) fn register(
    watcher: &mut Option<RecommendedWatcher>,
    root: &Path,
) -> Result<(), notify::Error> {
    watcher.as_mut().map_or_else(
        || Err(notify::Error::generic("no watcher")),
        |watcher| watcher.watch(root, RecursiveMode::Recursive),
    )
}

pub(crate) fn unregister(
    watcher: &mut Option<RecommendedWatcher>,
    root: &Path,
) -> Result<(), notify::Error> {
    watcher
        .as_mut()
        .map_or(Ok(()), |watcher| watcher.unwatch(root))
}

pub(crate) fn relocate(
    watcher: &mut Option<RecommendedWatcher>,
    from: &Path,
    to: &Path,
) -> Result<(), notify::Error> {
    let unregistered = unregister(watcher, from);
    let registered = register(watcher, to);
    unregistered.and(registered)
}

#[cfg(test)]
mod tests {
    use notify::RecommendedWatcher;

    use crate::library::notify::{register, unregister};

    #[test]
    fn registering_without_a_watcher_reports_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let error = register(&mut watcher, std::path::Path::new("/music"));
        assert!(error.is_err());
    }

    #[test]
    fn unregistering_without_a_watcher_is_a_no_op() {
        let mut watcher: Option<RecommendedWatcher> = None;
        assert!(unregister(&mut watcher, std::path::Path::new("/music")).is_ok());
    }

    #[test]
    #[ignore = "spins up a real OS filesystem watcher"]
    fn a_registered_watcher_can_be_unregistered() {
        let directory = tempfile::tempdir().unwrap();
        let mut watcher = Some(notify::recommended_watcher(|_| {}).unwrap());
        register(&mut watcher, directory.path()).unwrap();
        unregister(&mut watcher, directory.path()).unwrap();
    }
}
