use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub composer: Option<String>,
    pub comment: Option<String>,
    pub lyrics: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioFormat {
    pub format: Option<String>,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bits_per_sample: Option<u8>,
    pub channels: Option<u8>,
    pub replay_gain: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tagging {
    Listed,
    Read,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    path: PathBuf,
    duration: Option<Duration>,
    tags: Tags,
    audio_format: AudioFormat,
    display: Box<str>,
    tagging: Tagging,
}

#[bon::bon]
impl Track {
    #[builder]
    pub fn new(
        #[builder(into)] path: PathBuf,
        duration: Duration,
        tags: Tags,
        audio_format: AudioFormat,
    ) -> Self {
        let display = Self::compute_display(&path, &tags);
        Self {
            path,
            duration: Some(duration),
            tags,
            audio_format,
            display,
            tagging: Tagging::Read,
        }
    }

    #[must_use]
    pub fn listed(path: &Path) -> Self {
        Self {
            display: file_stem(path).into_boxed_str(),
            path: path.to_path_buf(),
            duration: None,
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
            tagging: Tagging::Listed,
        }
    }

    #[must_use]
    pub fn compute_display(path: &Path, tags: &Tags) -> Box<str> {
        let computed = match (&tags.title, &tags.artist) {
            (Some(title), Some(artist)) => format!("{artist} — {title}"),
            (Some(title), None) => title.clone(),
            _ => path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
        };
        computed.into_boxed_str()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    #[must_use]
    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    #[must_use]
    pub fn audio_format(&self) -> &AudioFormat {
        &self.audio_format
    }

    #[must_use]
    pub fn display(&self) -> &str {
        &self.display
    }

    #[must_use]
    pub fn tagging(&self) -> Tagging {
        self.tagging
    }

    #[must_use]
    pub fn song_title(&self) -> String {
        if let Some(title) = &self.tags.title {
            return title.clone();
        }
        file_stem(&self.path)
    }

    #[must_use]
    pub fn with_duration(&self, duration: Duration) -> Self {
        Self {
            path: self.path.clone(),
            duration: Some(duration),
            tags: self.tags.clone(),
            audio_format: self.audio_format.clone(),
            display: self.display.clone(),
            tagging: self.tagging,
        }
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.to_string_lossy().into_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}
