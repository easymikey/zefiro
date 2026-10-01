use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    AudioSettings,
    CustomSetting,
    Percent,
    PlaylistIndex,
    ThemeChoice,
    ThemeName,
    Track,
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
    pub playlist_index: Option<PlaylistIndex>,
    pub playlist_source: PlaylistSource,
    pub shuffle: Shuffle,
    pub audio: AudioSettings,
    pub theme: ThemeChoice,
    pub volume: Percent,
    pub themes: Vec<ThemeName>,
    pub custom_settings: Vec<CustomSetting>,
    pub toasts: Vec<String>,
}
