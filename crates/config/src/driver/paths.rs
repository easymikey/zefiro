use std::path::PathBuf;

use kernel::domain::theme::ThemeName;

#[must_use]
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub config_path: PathBuf,
    pub themes_dir: PathBuf,
    pub default_music_dir: Option<PathBuf>,
    pub theme_name: Option<ThemeName>,
    pub seen_texts: SeenTexts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenTexts {
    pub config: Option<String>,
}
