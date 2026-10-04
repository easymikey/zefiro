use config::driver::paths::ConfigPaths;
use library::dirs::LibraryDirs;

#[derive(Debug, Clone)]
pub struct StartupPaths {
    pub config: ConfigPaths,
    pub library: LibraryDirs,
}
