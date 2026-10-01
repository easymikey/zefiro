use std::{io, path::Path};

use kernel::domain::ConfigFile;

use crate::config::machine::ConfigMessage;

const THEME_EXTENSION: &str = "toml";

pub(crate) fn read(file: ConfigFile, path: &Path) -> ConfigMessage {
    match library::files::read_if_present(path) {
        Ok(text) => ConfigMessage::Read { file, text },
        Err(error) => ConfigMessage::Unreadable {
            file,
            detail: error.to_string(),
        },
    }
}

pub(crate) fn list_theme_names(dir: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str())
                    == Some(THEME_EXTENSION)
            })
            .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::ConfigFile;

    use crate::config::{
        disk::{list_theme_names, read},
        machine::ConfigMessage,
    };

    #[test]
    fn a_missing_file_reads_as_no_text() {
        let directory = tempfile::tempdir().unwrap();
        let message = read(ConfigFile::Theme, &directory.path().join("noir.toml"));

        assert!(matches!(message, ConfigMessage::Read { text: None, .. }));
    }

    #[test]
    fn listing_names_only_the_toml_files_by_stem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("noir.toml"), "").unwrap();
        std::fs::write(directory.path().join("notes.txt"), "").unwrap();

        let listed = list_theme_names(directory.path()).unwrap();

        assert_eq!(listed, vec!["noir".to_string()]);
    }

    #[test]
    fn a_missing_directory_lists_as_empty() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("themes");

        let listed = list_theme_names(&missing).unwrap();

        assert!(listed.is_empty());
    }

    #[test]
    fn a_directory_that_is_actually_a_file_is_unreadable() {
        let directory = tempfile::tempdir().unwrap();
        let not_a_directory = directory.path().join("themes");
        std::fs::write(&not_a_directory, "").unwrap();

        assert!(list_theme_names(&not_a_directory).is_err());
    }
}
