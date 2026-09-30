use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
};

use kernel::LibrarySubject;

use crate::{dirs::LibraryDirs, error::Error};

pub(crate) fn save(
    dirs: &LibraryDirs,
    favorites: &HashSet<PathBuf>,
) -> Result<(), Error> {
    let path = dirs.data_dir.join("favorites.json");
    crate::files::create_parent_dir(&path)
        .map_err(Error::write(LibrarySubject::Favorites, &path))?;
    let list: BTreeSet<&PathBuf> = favorites.iter().collect();
    let json = serde_json::to_string(&list)
        .map_err(Error::json(LibrarySubject::Favorites, &path))?;
    crate::files::write_atomic(&path, json.as_bytes())
        .map_err(Error::write(LibrarySubject::Favorites, &path))
}

pub(crate) fn load(dirs: &LibraryDirs) -> Result<HashSet<PathBuf>, Error> {
    let path = dirs.data_dir.join("favorites.json");
    let read = crate::files::read_if_present(&path);
    let Some(content) = read.map_err(Error::read(LibrarySubject::Favorites, &path))?
    else {
        return Ok(HashSet::new());
    };
    let list: Vec<PathBuf> = serde_json::from_str(&content)
        .map_err(Error::json(LibrarySubject::Favorites, &path))?;
    Ok(list.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use crate::{dirs::LibraryDirs, favorites};

    #[test]
    fn saved_favorites_load_back_and_a_second_save_overwrites_the_first() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        assert_eq!(favorites::load(&dirs).unwrap(), HashSet::new());

        let mut first = HashSet::new();
        first.insert(PathBuf::from("/music/a.flac"));
        first.insert(PathBuf::from("/music/b.flac"));
        favorites::save(&dirs, &first).unwrap();
        assert_eq!(favorites::load(&dirs).unwrap(), first);
        let raw =
            std::fs::read_to_string(dirs.data_dir.join("favorites.json")).unwrap();
        insta::assert_snapshot!(raw);

        let mut second = HashSet::new();
        second.insert(PathBuf::from("/music/c.flac"));
        favorites::save(&dirs, &second).unwrap();
        let loaded = favorites::load(&dirs).unwrap();
        assert_eq!(loaded, second);
        assert!(!loaded.contains(&PathBuf::from("/music/a.flac")));
    }
}
