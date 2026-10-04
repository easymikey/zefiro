use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    AppearanceSetting,
    AudioSettings,
    ConfigError,
    ConfigName,
    Percent,
    ThemeChoice,
    ThemeName,
    Track,
    ViewIndex,
    appearance::Appearance,
    playlist::PlaylistSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shuffle {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Default)]
pub struct Startup {
    pub music_dir: PathBuf,
    pub playlist_tracks: Vec<Arc<Track>>,
    pub playlist_index: Option<ViewIndex>,
    pub playlist_source: PlaylistSource,
    pub shuffle: Shuffle,
    pub audio: AudioSettings,
    pub appearance: Appearance,
    pub theme: ThemeChoice,
    pub volume: Percent,
    pub themes: Vec<ThemeName>,
    pub appearance_settings: Vec<AppearanceSetting>,
    pub errors: Vec<(ConfigName, ConfigError)>,
}
