use std::path::PathBuf;

use crate::error::LibraryError;

#[must_use]
#[derive(Debug, Clone)]
pub struct LibraryPaths {
    pub cache: PathBuf,
    pub data: PathBuf,
    pub playlists: PathBuf,
}

impl LibraryPaths {
    pub fn from_dirs() -> Result<Self, LibraryError> {
        let cache = dirs::cache_dir()
            .ok_or(LibraryError::NoDirectory)?
            .join("sifr");
        let data = dirs::data_dir()
            .ok_or(LibraryError::NoDirectory)?
            .join("sifr");
        let playlists = dirs::config_dir()
            .ok_or(LibraryError::NoDirectory)?
            .join("sifr")
            .join("playlists");
        Ok(Self {
            cache,
            data,
            playlists,
        })
    }
}

#[cfg(test)]
pub(crate) fn stub(root: &std::path::Path) -> LibraryPaths {
    LibraryPaths {
        cache: root.join("cache"),
        data: root.join("data"),
        playlists: root.join("playlists"),
    }
}
