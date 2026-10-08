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
