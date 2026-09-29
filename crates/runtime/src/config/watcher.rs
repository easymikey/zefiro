use std::path::{Path, PathBuf};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::ConfigPaths;

pub(crate) fn config_directory(paths: &ConfigPaths) -> PathBuf {
    match paths.appearance.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

pub(crate) trait FileWatch {
    fn watch(&mut self, path: &Path) -> Result<(), notify::Error>;
    fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error>;
}

impl FileWatch for RecommendedWatcher {
    fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
        Watcher::watch(self, path, RecursiveMode::Recursive)
    }

    fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
        Watcher::unwatch(self, path)
    }
}

impl<W: FileWatch> FileWatch for Option<W> {
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

pub(crate) fn register(
    watcher: &mut impl FileWatch,
    directory: &Path,
) -> Result<(), notify::Error> {
    if !directory.exists() {
        return Ok(());
    }
    watcher.watch(directory)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::RecommendedWatcher;

    use crate::config::{
        ConfigPaths,
        watcher::{config_directory, register},
    };

    fn paths(appearance: &str, themes: &str) -> ConfigPaths {
        ConfigPaths {
            config: None,
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
    fn registering_without_a_watcher_reports_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let directory = std::env::temp_dir();

        let outcome = register(&mut watcher, &directory);

        assert!(outcome.is_err());
    }

    #[test]
    fn registering_a_missing_directory_is_not_an_error() {
        let mut watcher: Option<RecommendedWatcher> = None;
        let missing = std::env::temp_dir().join("does-not-exist-config-watcher-test");

        let outcome = register(&mut watcher, &missing);

        assert!(outcome.is_ok());
    }
}
