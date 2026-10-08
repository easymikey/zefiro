use std::{
    io::{self, Write},
    path::Path,
};

pub fn read_if_present(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn create_parent_dir(path: &Path) -> io::Result<()> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
}

pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
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
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    staging.write_all(contents)?;
    staging.as_file().sync_all()?;
    staging.persist(&target).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::files::{read_if_present, write_atomic};

    #[test]
    fn a_missing_file_reads_as_nothing_rather_than_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("absent.json");
        assert_eq!(read_if_present(&path).unwrap(), None);
    }

    #[rstest]
    #[cfg(unix)]
    #[case::an_existing_target(Some(b"[1]".as_slice()))]
    #[case::a_dangling_link(None)]
    fn write_atomic_through_a_symlink_keeps_the_link_and_writes_the_target(
        #[case] before: Option<&[u8]>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let dotfiles = directory.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        if let Some(before) = before {
            std::fs::write(dotfiles.join("favorites.json"), before).unwrap();
        }
        let link = directory.path().join("favorites.json");
        std::os::unix::fs::symlink(
            std::path::Path::new("dotfiles").join("favorites.json"),
            &link,
        )
        .unwrap();

        write_atomic(&link, b"[2]").unwrap();

        assert!(link.is_symlink(), "the link stays a link");
        assert_eq!(
            std::fs::read(dotfiles.join("favorites.json")).unwrap(),
            b"[2]".to_vec()
        );
    }
}
