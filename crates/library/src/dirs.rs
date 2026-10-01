use std::path::PathBuf;

use crate::error::Error;

#[must_use]
#[derive(Debug, Clone)]
pub struct LibraryDirs {
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub playlists_dir: PathBuf,
}

impl LibraryDirs {
    pub fn user() -> Result<Self, Error> {
        let cache_dir = dirs::cache_dir().ok_or(Error::NoUserDirs)?.join("sifr");
        let data_dir = dirs::data_dir().ok_or(Error::NoUserDirs)?.join("sifr");
        let playlists_dir = dirs::config_dir()
            .ok_or(Error::NoUserDirs)?
            .join("sifr")
            .join("playlists");
        Ok(Self {
            cache_dir,
            data_dir,
            playlists_dir,
        })
    }

    pub fn under(root: &std::path::Path) -> Self {
        Self {
            cache_dir: root.join("cache"),
            data_dir: root.join("data"),
            playlists_dir: root.join("playlists"),
        }
    }
}
