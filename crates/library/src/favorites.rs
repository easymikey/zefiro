use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
};

use crate::{
    error::{LibraryError, Subject},
    paths::LibraryPaths,
};

pub(crate) fn save(
    paths: &LibraryPaths,
    favorites: &HashSet<PathBuf>,
) -> Result<(), LibraryError> {
    let path = paths.data.join("favorites.json");
    crate::files::create_parent(&path).map_err(|source| LibraryError::Write {
        subject: Subject::Favorites,
        path: path.clone(),
        source,
    })?;
    let list: BTreeSet<&PathBuf> = favorites.iter().collect();
    let json = serde_json::to_string(&list).map_err(|source| LibraryError::Json {
        subject: Subject::Favorites,
        path: path.clone(),
        source,
    })?;
    crate::files::persist(&path, json.as_bytes()).map_err(|source| {
        LibraryError::Write {
            subject: Subject::Favorites,
            path,
            source,
        }
    })
}

pub(crate) fn load(paths: &LibraryPaths) -> Result<HashSet<PathBuf>, LibraryError> {
    let path = paths.data.join("favorites.json");
    let read = crate::files::read_if_present(&path);
    let Some(content) = read.map_err(|source| LibraryError::Read {
        subject: Subject::Favorites,
        path: path.clone(),
        source,
    })?
    else {
        return Ok(HashSet::new());
    };
    let list: Vec<PathBuf> =
        serde_json::from_str(&content).map_err(|source| LibraryError::Json {
            subject: Subject::Favorites,
            path,
            source,
        })?;
    Ok(list.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use crate::{favorites, paths};

    #[test]
    fn load_save_round_trip_and_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());

        assert_eq!(favorites::load(&library_paths).unwrap(), HashSet::new());

        let mut first = HashSet::new();
        first.insert(PathBuf::from("/music/a.flac"));
        first.insert(PathBuf::from("/music/b.flac"));
        favorites::save(&library_paths, &first).unwrap();
        assert_eq!(favorites::load(&library_paths).unwrap(), first);
        let raw =
            std::fs::read_to_string(library_paths.data.join("favorites.json")).unwrap();
        insta::assert_snapshot!(raw);

        let mut second = HashSet::new();
        second.insert(PathBuf::from("/music/c.flac"));
        favorites::save(&library_paths, &second).unwrap();
        let loaded = favorites::load(&library_paths).unwrap();
        assert_eq!(loaded, second);
        assert!(!loaded.contains(&PathBuf::from("/music/a.flac")));
    }
}
