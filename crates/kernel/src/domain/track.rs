use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::domain::server::{MediaFetch, ServerAlbum, ServerName, ServerTrackId};

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
    Server {
        server_name: ServerName,
        server_track_id: ServerTrackId,
    },
}

impl TrackSource {
    #[must_use]
    pub fn local_path(&self) -> Option<&Path> {
        match self {
            TrackSource::Local(path) => Some(path),
            TrackSource::Server {
                server_name: _server_name,
                server_track_id: _server_track_id,
            } => None,
        }
    }

    fn name(&self) -> String {
        match self {
            TrackSource::Local(path) => path.file_stem().map_or_else(
                || path.to_string_lossy().into_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
            TrackSource::Server {
                server_name: _server_name,
                server_track_id,
            } => server_track_id.as_str().to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogRow {
    Album(ServerAlbum),
    Track(Arc<Track>),
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
            title: _title,
            tagging,
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
        Self::tagged(TrackSource::Local(path), duration, tags)
            .with_audio_format(audio_format)
    }

    #[must_use]
    pub fn tagged(source: TrackSource, duration: Duration, tags: Tags) -> Self {
        let display = Self::display_from(&source, &tags);
        let title = Self::title_from(&source, &tags);
        Self {
            source,
            tags,
            audio_format: AudioFormat::default(),
            display,
            title,
            tagging: Tagging::Tagged(duration),
        }
    }

    #[must_use]
    pub fn with_audio_format(self, audio_format: AudioFormat) -> Self {
        Self {
            audio_format,
            ..self
        }
    }

    #[must_use]
    pub fn listed(path: &Path) -> Self {
        Self::from(TrackSource::Local(path.to_path_buf()))
    }

    #[must_use]
    fn display_from(source: &TrackSource, tags: &Tags) -> Box<str> {
        let display = match (&tags.title, &tags.artist, source) {
            (Some(title), Some(artist), _source) => format!("{artist} — {title}"),
            (Some(title), None, _source) => title.clone(),
            (None, _artist, TrackSource::Local(path)) => path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            (
                None,
                _artist,
                TrackSource::Server {
                    server_name: _server_name,
                    server_track_id,
                },
            ) => server_track_id.as_str().to_owned(),
        };
        display.into_boxed_str()
    }

    fn title_from(source: &TrackSource, tags: &Tags) -> Box<str> {
        tags.title
            .clone()
            .unwrap_or_else(|| source.name())
            .into_boxed_str()
    }

    pub(crate) fn holds(&self, media_fetch: &MediaFetch) -> bool {
        matches!(
            &self.source,
            TrackSource::Server {
                server_name,
                server_track_id,
            } if *server_name == media_fetch.server_name
                && *server_track_id == media_fetch.server_track_id
        )
    }

    #[must_use]
    pub fn source(&self) -> &TrackSource {
        &self.source
    }

    #[must_use]
    pub fn local_path(&self) -> Option<&Path> {
        self.source.local_path()
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

impl From<TrackSource> for Track {
    fn from(source: TrackSource) -> Self {
        let name = source.name();
        Self {
            display: name.clone().into_boxed_str(),
            title: name.into_boxed_str(),
            source,
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
            tagging: Tagging::Listed(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cmp::Ordering, collections::HashSet, path::Path, time::Duration};

    use crate::domain::{
        server::{ServerName, ServerTrackId},
        track::{AudioFormat, Tags, Track, TrackParts, TrackSource},
    };

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

    #[test]
    fn a_server_source_orders_and_hashes_apart_from_a_local_one_with_the_same_text() {
        let local_source = TrackSource::Local("home".into());
        let server_source = TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("home"),
        };
        let sources: HashSet<&TrackSource> =
            [&local_source, &server_source].into_iter().collect();

        assert_ne!(local_source.cmp(&server_source), Ordering::Equal);
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn a_server_track_has_no_local_path() {
        let server_track = Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("tr-1"),
        });
        let local_track = Track::listed(Path::new("/music/a.flac"));

        assert_eq!(server_track.local_path(), None);
        assert_eq!(local_track.local_path(), Some(Path::new("/music/a.flac")));
    }
}
