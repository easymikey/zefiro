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

pub fn create_parent_dir(path: &Path) -> io::Result<()> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    staging.write_all(contents)?;
    staging.as_file().sync_all()?;
    staging.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::files::{create_parent_dir, read_if_present, write_atomic};

    #[test]
    fn a_missing_file_reads_as_nothing_rather_than_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("absent.json");
        assert_eq!(read_if_present(&path).unwrap(), None);
    }

    #[test]
    fn an_existing_file_reads_as_its_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("present.json");
        std::fs::write(&path, "[]").unwrap();
        assert_eq!(read_if_present(&path).unwrap(), Some("[]".to_string()));
    }

    #[test]
    fn create_parent_dir_makes_the_whole_chain_and_succeeds_when_repeated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("one").join("two").join("file.json");
        create_parent_dir(&path).unwrap();
        let parent = path.parent().unwrap();
        assert!(parent.is_dir());
        create_parent_dir(&path).unwrap();
    }

    #[test]
    fn write_atomic_writes_the_final_file_and_leaves_no_temp_behind() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("favorites.json");

        write_atomic(&path, b"[]").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"[]".to_vec());
        let leftovers: Vec<_> = std::fs::read_dir(directory.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn write_atomic_overwrites_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("favorites.json");
        std::fs::write(&path, b"[1]").unwrap();

        write_atomic(&path, b"[2]").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"[2]".to_vec());
    }
}
