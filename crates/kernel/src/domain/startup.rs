use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    appearance::AppearanceSettings,
    config::{ConfigError, ConfigName},
    index::ViewIndex,
    percent::Percent,
    playlist::PlaylistSource,
    setting_row::AppearanceSetting,
    settings::AudioSettings,
    theme::{ThemeChoice, ThemeName},
    track::Track,
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
    pub appearance: AppearanceSettings,
    pub theme: ThemeChoice,
    pub volume: Percent,
    pub themes: Vec<ThemeName>,
    pub appearance_settings: Vec<AppearanceSetting>,
    pub errors: Vec<(ConfigName, ConfigError)>,
}
