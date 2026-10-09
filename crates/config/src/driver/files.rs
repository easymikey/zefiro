use std::{io, io::Write, path::Path};

use kernel::domain::{
    config::{ConfigError, ConfigName, Diagnostic},
    io_error::IoError,
};

pub(crate) fn read_error(name: ConfigName, error: &io::Error) -> ConfigError {
    ConfigError::Read {
        name,
        error: IoError::from(error.kind()),
    }
}

pub(crate) fn parent_dir(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

pub(crate) fn store(path: &Path, contents: &[u8]) -> Result<(), IoError> {
    let parent = parent_dir(path).ok_or(IoError::Missing)?;
    std::fs::create_dir_all(parent).map_err(|error| IoError::from(error.kind()))?;
    write_atomic(path, contents).map_err(|error| IoError::from(error.kind()))
}

pub(crate) fn save(
    path: &Path,
    produce: impl FnOnce(&str) -> Result<String, crate::error::Error>,
) -> Result<String, ConfigError> {
    let old_text = read_if_present(path)
        .map_err(|error| save_failed(IoError::from(error.kind())))?;
    let text = produce(old_text.as_deref().unwrap_or(""))
        .map_err(|error| ConfigError::from(Diagnostic::from_error(&error)))?;
    store(path, text.as_bytes()).map_err(save_failed)?;
    Ok(text)
}

pub(crate) fn save_failed(error: IoError) -> ConfigError {
    ConfigError::Save {
        name: ConfigName::Config,
        error,
    }
}

pub(crate) fn read_if_present(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let target = match std::fs::canonicalize(path) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound && path.is_symlink() => {
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .join(std::fs::read_link(path)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let mut staging = tempfile::NamedTempFile::new_in(
        parent_dir(&target).unwrap_or_else(|| Path::new(".")),
    )?;
    staging.write_all(contents)?;
    staging.as_file().sync_all()?;
    staging.persist(&target).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::driver::files::store;

    #[rstest]
    #[cfg(unix)]
    #[case::an_existing_target(Some(b"old = 1".as_slice()))]
    #[case::a_dangling_link(None)]
    fn store_through_a_symlink_keeps_the_link_and_writes_the_target(
        #[case] before: Option<&[u8]>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let dotfiles = directory.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        if let Some(before) = before {
            std::fs::write(dotfiles.join("config.toml"), before).unwrap();
        }
        let link = directory.path().join("config.toml");
        std::os::unix::fs::symlink(
            std::path::Path::new("dotfiles").join("config.toml"),
            &link,
        )
        .unwrap();

        store(&link, b"new = 2").unwrap();

        assert!(link.is_symlink(), "the link stays a link");
        assert_eq!(
            std::fs::read(dotfiles.join("config.toml")).unwrap(),
            b"new = 2".to_vec()
        );
    }
}
