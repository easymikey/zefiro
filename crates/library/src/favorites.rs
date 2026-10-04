use std::{collections::BTreeSet, path::PathBuf};

use kernel::{
    domain::{favorites::Favorites, track::TrackRef},
    message::LibrarySubject,
};

use crate::{dirs::LibraryDirs, error::Error};

pub(crate) fn save(dirs: &LibraryDirs, favorites: &Favorites) -> Result<(), Error> {
    let path = dirs.data_dir.join("favorites.json");
    crate::files::create_parent_dir(&path)
        .map_err(Error::io(LibrarySubject::Favorites, &path))?;
    let list: BTreeSet<&PathBuf> = favorites
        .iter()
        .map(|track| {
            let TrackRef::Local(track_path) = track;
            track_path
        })
        .collect();
    let json = serde_json::to_string(&list)
        .map_err(Error::json(LibrarySubject::Favorites, &path))?;
    crate::files::write_atomic(&path, json.as_bytes())
        .map_err(Error::io(LibrarySubject::Favorites, &path))
}

pub(crate) fn load(dirs: &LibraryDirs) -> Result<Favorites, Error> {
    let path = dirs.data_dir.join("favorites.json");
    let read = crate::files::read_if_present(&path);
    let Some(content) = read.map_err(Error::io(LibrarySubject::Favorites, &path))?
    else {
        return Ok(Favorites::default());
    };
    let list: Vec<PathBuf> = serde_json::from_str(&content)
        .map_err(Error::json(LibrarySubject::Favorites, &path))?;
    Ok(list.into_iter().map(TrackRef::Local).collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::domain::{favorites::Favorites, track::TrackRef};

    use crate::{dirs::LibraryDirs, favorites};

    #[test]
    fn saved_favorites_load_back_and_a_second_save_overwrites_the_first() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());

        assert_eq!(favorites::load(&dirs).unwrap(), Favorites::default());

        let first: Favorites = ["/music/a.flac", "/music/b.flac"]
            .map(PathBuf::from)
            .map(TrackRef::Local)
            .into_iter()
            .collect();
        favorites::save(&dirs, &first).unwrap();
        assert_eq!(favorites::load(&dirs).unwrap(), first);
        let raw =
            std::fs::read_to_string(dirs.data_dir.join("favorites.json")).unwrap();
        insta::assert_snapshot!(raw);

        let second: Favorites = [TrackRef::Local("/music/c.flac".into())]
            .into_iter()
            .collect();
        favorites::save(&dirs, &second).unwrap();
        let loaded = favorites::load(&dirs).unwrap();
        assert_eq!(loaded, second);
        assert!(!loaded.is_favorite(&TrackRef::Local("/music/a.flac".into())));
    }
}
