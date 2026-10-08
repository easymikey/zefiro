use std::path::Path;

use crate::error::Error;

pub(crate) fn move_to_trash(path: &Path) -> Result<(), Error> {
    let missing = std::fs::symlink_metadata(path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    if missing {
        return Ok(());
    }
    trash::delete(path).map_err(|source| Error::Trash {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use crate::trash::move_to_trash;

    #[test]
    fn move_to_trash_missing_file_is_ok_without_reaching_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-existed.flac");

        assert!(!missing.exists());
        assert!(move_to_trash(&missing).is_ok());
    }
}
