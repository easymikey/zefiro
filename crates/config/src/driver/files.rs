use std::{io, io::Write, path::Path};

use kernel::domain::io_error::IoError;

pub(crate) fn store(path: &Path, contents: &[u8]) -> Result<(), IoError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(IoError::Missing)?;
    std::fs::create_dir_all(parent).map_err(|error| IoError::from(error.kind()))?;
    write_atomic(parent, path, contents).map_err(|error| IoError::from(error.kind()))
}

pub(crate) fn read_if_present(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_atomic(parent: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    staging.write_all(contents)?;
    staging.as_file().sync_all()?;
    staging.persist(path).map_err(|error| error.error)?;
    Ok(())
}
