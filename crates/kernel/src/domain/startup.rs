use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::domain::{
    Crossfade,
    CustomSetting,
    OutputDevice,
    Percent,
    PlaylistIndex,
    Replaygain,
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
    pub crossfade: Crossfade,
    pub replaygain: Replaygain,
    pub output_device: OutputDevice,
    pub sleep_presets: Box<[Duration]>,
    pub theme: ThemeChoice,
    pub volume: Percent,
    pub themes: Vec<ThemeName>,
    pub custom_settings: Vec<CustomSetting>,
    pub toasts: Vec<String>,
}
