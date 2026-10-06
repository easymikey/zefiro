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
    pub track_number: Option<u32>,
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
    pub decibels: Option<Decibels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tagging {
    Listed(Option<Duration>),
    Tagged(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrackSource {
    Local(PathBuf),
}

#[derive(Clone, PartialEq)]
pub struct Track {
    source: TrackSource,
    tags: Tags,
    audio_format: AudioFormat,
    display: Box<str>,
    title: Box<str>,
    tagging: Tagging,
}

impl std::fmt::Debug for Track {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            source,
            tags,
            audio_format,
            display,
            tagging,
            ..
        } = self;
        formatter
            .debug_struct("Track")
            .field("source", source)
            .field("tags", tags)
            .field("audio_format", audio_format)
            .field("display", display)
            .field("tagging", tagging)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackParts {
    pub path: PathBuf,
    pub duration: Duration,
    pub tags: Tags,
    pub audio_format: AudioFormat,
}

impl Track {
    #[must_use]
    pub fn new(track_parts: TrackParts) -> Self {
        let TrackParts {
            path,
            duration,
            tags,
            audio_format,
        } = track_parts;
        let display = Self::display_from(&path, &tags);
        let title = Self::title_from(&path, &tags);
        Self {
            source: TrackSource::Local(path),
            tags,
            audio_format,
            display,
            title,
            tagging: Tagging::Tagged(duration),
        }
    }

    #[must_use]
    pub fn listed(path: &Path) -> Self {
        Self {
            display: file_stem(path).into_boxed_str(),
            title: file_stem(path).into_boxed_str(),
            source: TrackSource::Local(path.to_path_buf()),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
            tagging: Tagging::Listed(None),
        }
    }

    #[must_use]
    fn display_from(path: &Path, tags: &Tags) -> Box<str> {
        let display = match (&tags.title, &tags.artist) {
            (Some(title), Some(artist)) => format!("{artist} — {title}"),
            (Some(title), None) => title.clone(),
            _ => path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
        };
        display.into_boxed_str()
    }

    fn title_from(path: &Path, tags: &Tags) -> Box<str> {
        tags.title
            .clone()
            .unwrap_or_else(|| file_stem(path))
            .into_boxed_str()
    }

    #[must_use]
    pub fn source(&self) -> &TrackSource {
        &self.source
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        let TrackSource::Local(path) = &self.source;
        path
    }

    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        match self.tagging {
            Tagging::Listed(duration) => duration,
            Tagging::Tagged(duration) => Some(duration),
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
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub(crate) fn with_duration(&self, duration: Duration) -> Self {
        let tagging = match self.tagging {
            Tagging::Listed(_) => Tagging::Listed(Some(duration)),
            Tagging::Tagged(_) => Tagging::Tagged(duration),
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

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use crate::domain::track::{AudioFormat, Tags, Track, TrackParts};

    #[test]
    fn a_tagged_track_shows_its_tag_title_as_the_title() {
        let track = Track::new(TrackParts {
            path: "/music/file-name.mp3".into(),
            duration: Duration::from_secs(1),
            tags: Tags {
                title: Some("Song".to_owned()),
                ..Tags::default()
            },
            audio_format: AudioFormat::default(),
        });

        assert_eq!(track.title(), "Song");
    }

    #[test]
    fn an_untagged_track_shows_its_file_stem_as_the_title() {
        let built_track = Track::new(TrackParts {
            path: "/music/file-name.mp3".into(),
            duration: Duration::from_secs(1),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        });
        let listed = Track::listed(Path::new("/music/file-name.mp3"));

        assert_eq!(built_track.title(), "file-name");
        assert_eq!(listed.title(), "file-name");
    }
}
