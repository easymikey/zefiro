use std::path::{Path, PathBuf};

use notify::{RecommendedWatcher, RecursiveMode};

use crate::config::ConfigPaths;

pub(crate) fn config_directory(paths: &ConfigPaths) -> PathBuf {
    match paths.appearance.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
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

pub(crate) fn watch_if_present(
    watcher: &mut impl Watcher,
    directory: &Path,
) -> Result<(), notify::Error> {
    if !directory.exists() {
        return Ok(());
    }
    watcher.watch(directory)
}

pub(crate) fn rewatch(
    watcher: &mut impl Watcher,
    from: &Path,
    to: &Path,
) -> Result<(), notify::Error> {
    let unwatched = watcher.unwatch(from);
    let watched = watcher.watch(to);
    unwatched.and(watched)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use notify::RecommendedWatcher;

    use crate::{
        config::ConfigPaths,
        watcher::{Watcher, config_directory, rewatch, watch_if_present},
    };

    #[derive(Default)]
    struct FakeWatch {
        watched: Vec<PathBuf>,
    }

    impl Watcher for FakeWatch {
        fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
            self.watched.retain(|watched| watched != path);
            Ok(())
        }
    }

    fn paths(appearance: &str, themes: &str) -> ConfigPaths {
        ConfigPaths {
            config: PathBuf::from("config.toml"),
            appearance: PathBuf::from(appearance),
            themes: PathBuf::from(themes),
            theme: None,
            seen: crate::config::SeenTexts::default(),
        }
    }

    #[test]
    fn the_config_directory_is_the_appearance_files_parent() {
        let paths = paths("/config/sifr-ui.toml", "/config/themes");

        assert_eq!(config_directory(&paths), PathBuf::from("/config"));
    }

    #[test]
    fn a_bare_appearance_filename_resolves_to_the_current_directory() {
        let paths = paths("sifr-ui.toml", "themes");

        assert_eq!(config_directory(&paths), PathBuf::from("."));
    }

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

    #[test]
    fn a_watched_root_can_be_unwatched() {
        let mut watcher = FakeWatch::default();
        watcher.watch(Path::new("/music")).unwrap();
        assert_eq!(watcher.watched, vec![PathBuf::from("/music")]);
        watcher.unwatch(Path::new("/music")).unwrap();
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn rewatching_moves_the_watch_to_the_new_root() {
        let mut watcher = FakeWatch::default();
        watcher.watch(Path::new("/music")).unwrap();
        rewatch(&mut watcher, Path::new("/music"), Path::new("/tunes")).unwrap();
        assert_eq!(watcher.watched, vec![PathBuf::from("/tunes")]);
    }
}
