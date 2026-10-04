use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Default, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decibels(pub f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kbps(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hertz(pub u32);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioFormat {
    pub format: Option<String>,
    pub bitrate: Option<Kbps>,
    pub sample_rate: Option<Hertz>,
    pub bits_per_sample: Option<u8>,
    pub channels: Option<u8>,
    pub replay_gain: Option<Decibels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tagging {
    Listed(Option<Duration>),
    Read(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrackRef {
    Local(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    source: TrackRef,
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
            source: TrackRef::Local(path),
            tags,
            audio_format,
            display,
            tagging: Tagging::Read(duration),
        }
    }

    #[must_use]
    pub fn listed(path: &Path) -> Self {
        Self {
            display: file_stem(path).into_boxed_str(),
            source: TrackRef::Local(path.to_path_buf()),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
            tagging: Tagging::Listed(None),
        }
    }

    #[must_use]
    fn compute_display(path: &Path, tags: &Tags) -> Box<str> {
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
    pub fn source(&self) -> &TrackRef {
        &self.source
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        let TrackRef::Local(path) = &self.source;
        path
    }

    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        match self.tagging {
            Tagging::Listed(duration) => duration,
            Tagging::Read(duration) => Some(duration),
        }
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
        file_stem(self.path())
    }

    #[must_use]
    pub(crate) fn with_duration(&self, duration: Duration) -> Self {
        let tagging = match self.tagging {
            Tagging::Listed(_) => Tagging::Listed(Some(duration)),
            Tagging::Read(_) => Tagging::Read(duration),
        };
        Self {
            tagging,
            ..self.clone()
        }
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.to_string_lossy().into_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}
