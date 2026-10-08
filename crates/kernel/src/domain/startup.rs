use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    appearance::AppearanceSettings,
    config::{ConfigError, ConfigName},
    index::ViewIndex,
    keymap::KeymapOverrides,
    percent::Percent,
    playlist::PlaylistSource,
    server::Account,
    settings::AudioSettings,
    theme::{ThemeChoice, ThemeName},
    track::Track,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shuffle {
    #[default]
    Off,
    On,
}

#[derive(Debug, Clone, Default)]
pub struct Startup {
    pub music_dir: PathBuf,
    pub playlist_tracks: Vec<Arc<Track>>,
    pub playlist_index: Option<ViewIndex>,
    pub playlist_source: PlaylistSource,
    pub shuffle: Shuffle,
    pub audio_settings: AudioSettings,
    pub appearance_settings: AppearanceSettings,
    pub theme_choice: ThemeChoice,
    pub volume: Percent,
    pub keymap_overrides: KeymapOverrides,
    pub theme_names: Vec<ThemeName>,
    pub errors: Vec<(ConfigName, ConfigError)>,
    pub accounts: Vec<Account>,
}
