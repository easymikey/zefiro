use std::path::Path;

use crate::config::watcher::FileWatch;

pub(crate) fn register(
    watcher: &mut impl FileWatch,
    root: &Path,
) -> Result<(), notify::Error> {
    watcher.watch(root)
}

pub(crate) fn unregister(
    watcher: &mut impl FileWatch,
    root: &Path,
) -> Result<(), notify::Error> {
    watcher.unwatch(root)
}

pub(crate) fn relocate(
    watcher: &mut impl FileWatch,
    from: &Path,
    to: &Path,
) -> Result<(), notify::Error> {
    let unregistered = unregister(watcher, from);
    let registered = register(watcher, to);
    unregistered.and(registered)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use notify::RecommendedWatcher;

    use crate::{
        config::watcher::FileWatch,
        library::notify::{register, relocate, unregister},
    };

    #[derive(Default)]
    struct FakeWatch {
        watched: Vec<PathBuf>,
    }

    impl FileWatch for FakeWatch {
        fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.retain(|watched| watched != path);
            Ok(())
        }
    }

    #[test]
    fn registering_without_a_watcher_reports_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let error = register(&mut watcher, Path::new("/music"));
        assert!(error.is_err());
    }

    #[test]
    fn unregistering_without_a_watcher_is_a_no_op() {
        let mut watcher: Option<RecommendedWatcher> = None;
        assert!(unregister(&mut watcher, Path::new("/music")).is_ok());
    }

    #[test]
    fn a_registered_watcher_can_be_unregistered() {
        let mut watcher = FakeWatch::default();
        register(&mut watcher, Path::new("/music")).unwrap();
        assert_eq!(watcher.watched, vec![PathBuf::from("/music")]);
        unregister(&mut watcher, Path::new("/music")).unwrap();
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn relocating_moves_the_watch_to_the_new_root() {
        let mut watcher = FakeWatch::default();
        register(&mut watcher, Path::new("/music")).unwrap();
        relocate(&mut watcher, Path::new("/music"), Path::new("/tunes")).unwrap();
        assert_eq!(watcher.watched, vec![PathBuf::from("/tunes")]);
    }
}
