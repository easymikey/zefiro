use std::path::Path;

use crate::error::LibraryError;

pub(crate) fn move_to_trash(path: &Path) -> Result<(), LibraryError> {
    move_to_trash_with(path, |doomed: &Path| trash::delete(doomed))
}

pub(crate) fn move_to_trash_with(
    path: &Path,
    mut delete: impl FnMut(&Path) -> Result<(), trash::Error>,
) -> Result<(), LibraryError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) | Ok(_) => delete(path).map_err(|source| LibraryError::Trash {
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
        let mut asked: Vec<PathBuf> = Vec::new();

        assert!(!missing.exists());
        assert!(
            move_to_trash_with(&missing, |path: &Path| {
                asked.push(path.to_path_buf());
                Ok(())
            })
            .is_ok()
        );
        assert!(asked.is_empty());
    }

    #[test]
    fn move_to_trash_hands_a_file_that_is_there_to_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        let doomed = dir.path().join("doomed.flac");
        std::fs::write(&doomed, b"stub").unwrap();
        let mut asked: Vec<PathBuf> = Vec::new();

        move_to_trash_with(&doomed, |path: &Path| {
            asked.push(path.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(asked, vec![doomed]);
    }
}
