use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::domain::{
    Crossfade,
    CustomSetting,
    Percent,
    PlaylistIndex,
    Replaygain,
    Track,
    playlist::PlaylistSource,
};

#[derive(Debug, Clone, Default)]
pub struct Startup {
    pub music_dir: PathBuf,
    pub playlist_tracks: Vec<Arc<Track>>,
    pub playlist_index: Option<PlaylistIndex>,
    pub playlist_source: PlaylistSource,
    pub shuffle_order: Option<Vec<usize>>,
    pub crossfade: Crossfade,
    pub replaygain: Replaygain,
    pub output_device: Option<String>,
    pub sleep_presets: Box<[Duration]>,
    pub theme: String,
    pub volume: Percent,
    pub themes: Vec<String>,
    pub custom_rows: Vec<CustomSetting>,
}
