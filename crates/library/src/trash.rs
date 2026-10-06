use std::path::Path;

use crate::error::Error;

pub(crate) fn move_to_trash(path: &Path) -> Result<(), Error> {
    move_to_trash_with(path, |doomed: &Path| trash::delete(doomed))
}

pub(crate) fn move_to_trash_with(
    path: &Path,
    mut delete: impl FnMut(&Path) -> Result<(), trash::Error>,
) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) | Ok(_) => delete(path).map_err(|source| Error::Trash {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::trash::move_to_trash_with;

    #[test]
    fn move_to_trash_missing_file_is_ok_without_reaching_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-existed.flac");
        let mut wanted_paths: Vec<PathBuf> = Vec::new();

        assert!(!missing.exists());
        assert!(
            move_to_trash_with(&missing, |path: &Path| {
                wanted_paths.push(path.to_path_buf());
                Ok(())
            })
            .is_ok()
        );
        assert!(wanted_paths.is_empty());
    }

    #[test]
    fn move_to_trash_hands_a_file_that_is_there_to_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        let doomed = dir.path().join("doomed.flac");
        std::fs::write(&doomed, b"stub").unwrap();
        let mut wanted_paths: Vec<PathBuf> = Vec::new();

        move_to_trash_with(&doomed, |path: &Path| {
            wanted_paths.push(path.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(wanted_paths, vec![doomed]);
    }
}
