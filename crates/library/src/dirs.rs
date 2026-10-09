use std::path::{Path, PathBuf};

use crate::error::Error;

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDirs {
    pub(crate) cache_dir: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) playlists_dir: PathBuf,
}

impl LibraryDirs {
    pub fn user() -> Result<Self, Error> {
        let user_cache_dir = dirs::cache_dir().ok_or(Error::NoUserDirs)?;
        let user_data_dir = dirs::data_dir().ok_or(Error::NoUserDirs)?;
        let user_config_dir = dirs::config_dir().ok_or(Error::NoUserDirs)?;
        Ok(Self::new(&user_cache_dir, &user_data_dir, &user_config_dir))
    }

    pub fn new(
        user_cache_dir: &Path,
        user_data_dir: &Path,
        user_config_dir: &Path,
    ) -> Self {
        Self {
            cache_dir: user_cache_dir.join("zefiro"),
            data_dir: user_data_dir.join("zefiro"),
            playlists_dir: user_config_dir.join("zefiro").join("playlists"),
        }
    }

    #[must_use]
    pub fn media_dir(&self) -> PathBuf {
        self.cache_dir.join("media")
    }

    #[must_use]
    pub fn reports_path(&self) -> PathBuf {
        self.cache_dir.join("reports.json")
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::dirs::LibraryDirs;

    fn library_dirs() -> LibraryDirs {
        LibraryDirs::new(
            Path::new("/user/cache"),
            Path::new("/user/data"),
            Path::new("/user/config"),
        )
    }

    #[test]
    fn new_puts_each_dir_in_the_app_folder_of_its_user_dir() {
        assert_eq!(
            library_dirs(),
            LibraryDirs {
                cache_dir: PathBuf::from("/user/cache/zefiro"),
                data_dir: PathBuf::from("/user/data/zefiro"),
                playlists_dir: PathBuf::from("/user/config/zefiro/playlists"),
            }
        );
    }

    #[test]
    fn the_media_dir_lies_in_the_cache_dir() {
        assert_eq!(
            library_dirs().media_dir(),
            Path::new("/user/cache/zefiro/media")
        );
    }
}
