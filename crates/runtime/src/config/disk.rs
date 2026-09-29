use std::{io, path::Path};

use crate::config::watch::{ConfigWatchMessage, WatchedFile, config_file};

const THEME_EXTENSION: &str = "toml";

pub(crate) fn read(file: WatchedFile, path: &Path) -> ConfigWatchMessage {
    match library::files::read_if_present(path) {
        Ok(text) => ConfigWatchMessage::Observed { file, text },
        Err(error) => ConfigWatchMessage::Unreadable {
            file: config_file(file),
            detail: error.to_string(),
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    Names(Vec<String>),
    Unreadable(String),
}

pub(crate) fn list_theme_names(dir: &Path) -> Listing {
    match std::fs::read_dir(dir) {
        Ok(entries) => Listing::Names(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().and_then(|extension| extension.to_str())
                        == Some(THEME_EXTENSION)
                })
                .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
                .collect(),
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Listing::Names(Vec::new())
        }
        Err(error) => Listing::Unreadable(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{
        disk::{Listing, list_theme_names, read},
        watch::{ConfigWatchMessage, WatchedFile},
    };

    #[test]
    fn a_missing_file_reads_as_no_text() {
        let directory = tempfile::tempdir().unwrap();
        let message = read(WatchedFile::Theme, &directory.path().join("noir.toml"));

        assert!(matches!(
            message,
            ConfigWatchMessage::Observed { text: None, .. }
        ));
    }

    fn names(listing: Listing) -> Option<Vec<String>> {
        match listing {
            Listing::Names(names) => Some(names),
            Listing::Unreadable(_) => None,
        }
    }

    #[test]
    fn listing_names_only_the_toml_files_by_stem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("noir.toml"), "").unwrap();
        std::fs::write(directory.path().join("notes.txt"), "").unwrap();

        let listed = names(list_theme_names(directory.path())).unwrap();

        assert_eq!(listed, vec!["noir".to_string()]);
    }

    #[test]
    fn a_missing_directory_lists_as_empty() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("themes");

        let listed = names(list_theme_names(&missing)).unwrap();

        assert!(listed.is_empty());
    }

    #[test]
    fn a_directory_that_is_actually_a_file_is_unreadable() {
        let directory = tempfile::tempdir().unwrap();
        let not_a_directory = directory.path().join("themes");
        std::fs::write(&not_a_directory, "").unwrap();

        assert!(matches!(
            list_theme_names(&not_a_directory),
            Listing::Unreadable(_)
        ));
    }
}
