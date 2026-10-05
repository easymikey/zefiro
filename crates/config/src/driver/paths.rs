use std::path::PathBuf;

use kernel::domain::theme::ThemeName;

#[must_use]
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub config: PathBuf,
    pub appearance: PathBuf,
    pub themes: PathBuf,
    pub default_music_dir: Option<PathBuf>,
    pub theme: Option<ThemeName>,
    pub seen: SeenTexts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenTexts {
    pub appearance: Option<String>,
    pub config: Option<String>,
}
