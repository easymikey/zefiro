use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{
    ChordPrefix,
    ConfigError,
    ConfigFile,
    CustomSetting,
    Direction,
    Driver,
    DriverError,
    HistoryEntry,
    KeyPress,
    KeymapOverrides,
    ListedDevice,
    OutputDevice,
    OverlayName,
    Percent,
    PlaylistIndex,
    Revision,
    SettingRow,
    StreamError,
    ThemeName,
    Toast,
    Track,
    playlist::PlaylistFileName,
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Message {
    Overlay(OverlayRequest),
    Adjust {
        row: SettingRow,
        direction: Direction,
    },
    Workspace(WorkspaceRequest),
    Playback(PlaybackRequest),
    Browse(BrowseRequest),
    Queue(QueueRequest),
    Loaded(PlaylistRequest),
    Library(LibraryEvent),
    Config(ConfigEvent),
    Audio(AudioEvent),
    Macos(MacosEvent),
    Elapsed(Timer),
    Driver(Driver, DriverMessage),
    Key(KeyPress),
    Viewport {
        visible_rows: usize,
    },
    Quit,
}

impl From<AudioEvent> for Message {
    fn from(event: AudioEvent) -> Self {
        Message::Audio(event)
    }
}

impl From<MacosEvent> for Message {
    fn from(event: MacosEvent) -> Self {
        Message::Macos(event)
    }
}

impl From<LibraryEvent> for Message {
    fn from(event: LibraryEvent) -> Self {
        Message::Library(event)
    }
}

impl From<ConfigEvent> for Message {
    fn from(event: ConfigEvent) -> Self {
        Message::Config(event)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Timer {
    Toast(Revision),
    Sleep(Revision),
    Mark(Revision),
    Restart(Driver),
}

#[derive(Debug, Clone, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DriverMessage {
    Died(DriverError),
    Stopped,
    Full,
    Rejected { input: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum OverlayRequest {
    Open(OverlayName),
    Close,
    Confirm,
    Search(SearchRequest),
    Settings(SettingsRowRequest),
    Text(TextRequest),
    Jump(TextRequest),
    History(HistoryRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SearchRequest {
    Edit(SearchEdit),
    Navigate(Direction),
    Enqueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEdit {
    Char(char),
    Backspace,
    DeleteWord,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRowRequest {
    Navigate(Direction),
    Adjust(Direction),
    Activate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRequest {
    Char(char),
    Backspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    Navigate(Direction),
    Top,
    Bottom,
    Enqueue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceRequest {
    ShowToast(Toast),
    ClearToast,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigEvent {
    KeymapReloaded(Box<KeymapOverrides>),
    ThemeReloaded(ThemeName),
    ThemesLoaded(Vec<ThemeName>),
    MusicDirReloaded(PathBuf),
    CustomRowsReloaded(Vec<CustomSetting>),
    SourceFailed { source: ConfigFile, text: String },
    SourceRecovered(ConfigFile),
    Error(ConfigError),
}

#[derive(Debug, Clone, Copy, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlaybackRequest {
    Toggle,
    Play,
    Pause,
    SeekForward,
    SeekBack,
    Hold,
    Release,
    Stop,
    Next,
    Previous,
    SeekBy { seconds: i64 },
    NudgeVolume { steps: i8 },
    ToggleShuffle,
    CycleRepeat,
    CycleSleep,
    AbMark,
    NudgeSpeed { steps: i8 },
    SeekTo(Duration),
    SeekFraction(SeekTenths),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeekTenths(u8);

impl SeekTenths {
    #[must_use]
    pub fn tenths(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("seek fraction {tenths} is out of range (must be 0..=9)")]
pub struct SeekTenthsOutOfRange {
    pub tenths: u8,
}

impl TryFrom<u8> for SeekTenths {
    type Error = SeekTenthsOutOfRange;

    fn try_from(tenths: u8) -> Result<Self, Self::Error> {
        if tenths <= 9 {
            Ok(Self(tenths))
        } else {
            Err(SeekTenthsOutOfRange { tenths })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseRequest {
    Trash(PlaylistIndex),
    SavePlaylist(PlaylistFileName),
    ChordPrefix(ChordPrefix),
    CursorBy { rows: i64 },
    Top,
    Bottom,
    PlaySelected,
    CycleSort,
    Rescan,
    ToggleFavorite,
    CursorTo(PlaylistIndex),
    PageBy(Direction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum QueueRequest {
    Enqueue,
    EnqueueTrack(PlaylistIndex),
    PlayNext,
    Dequeue,
    MoveInQueue(Direction),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlaylistRequest {
    JumpTo(PlaylistIndex),
    ShuffleRolled(Vec<usize>),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LibraryEvent {
    Loaded {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    Listed {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    Tagged {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    FavoritesLoaded(HashSet<PathBuf>),
    HistoryLoaded(Vec<HistoryEntry>),
    Error(LibraryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySubject {
    Scan,
    Playlist,
    History,
    Favorites,
    Trash,
    Cache,
    Watch,
}

impl std::fmt::Display for LibrarySubject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            LibrarySubject::Scan => "a scan",
            LibrarySubject::Playlist => "the playlist file",
            LibrarySubject::History => "the history file",
            LibrarySubject::Favorites => "the favorites file",
            LibrarySubject::Trash => "the trash",
            LibrarySubject::Cache => "the cache",
            LibrarySubject::Watch => "the watched folder",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    Missing,
    Denied,
    Malformed,
    Full,
    Other,
}

impl std::fmt::Display for IoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            IoError::Missing => "not found",
            IoError::Denied => "permission denied",
            IoError::Malformed => "corrupt data",
            IoError::Full => "disk full",
            IoError::Other => "an unknown error",
        };
        formatter.write_str(label)
    }
}

impl From<std::io::ErrorKind> for IoError {
    fn from(kind: std::io::ErrorKind) -> Self {
        if kind == std::io::ErrorKind::NotFound {
            IoError::Missing
        } else if kind == std::io::ErrorKind::PermissionDenied {
            IoError::Denied
        } else if kind == std::io::ErrorKind::StorageFull {
            IoError::Full
        } else {
            IoError::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryError {
    #[error("Could not read {subject} ({}): {kind}", path.display())]
    File {
        subject: LibrarySubject,
        path: PathBuf,
        kind: IoError,
    },
    #[error("no library directory")]
    NoUserDirs,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioEvent {
    Playhead(Duration),
    TrackChanged,
    Ended,
    Loaded { total: Option<Duration> },
    Error(AudioError),
    DevicesListed(Vec<ListedDevice>),
    DeviceFellBack(OutputDevice),
}

#[derive(Debug, Clone, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MacosEvent {
    Volume(Percent),
    OutputRouteChanged,
    HardwareWatchError(String),
    MediaKey(Gesture),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    SeekForward,
    SeekBack,
    Scrub(Duration),
}

impl From<Gesture> for PlaybackRequest {
    fn from(gesture: Gesture) -> Self {
        match gesture {
            Gesture::Play => PlaybackRequest::Play,
            Gesture::Pause => PlaybackRequest::Pause,
            Gesture::Toggle => PlaybackRequest::Toggle,
            Gesture::Stop => PlaybackRequest::Stop,
            Gesture::Next => PlaybackRequest::Next,
            Gesture::Previous => PlaybackRequest::Previous,
            Gesture::SeekForward => PlaybackRequest::SeekForward,
            Gesture::SeekBack => PlaybackRequest::SeekBack,
            Gesture::Scrub(position) => PlaybackRequest::SeekTo(position),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Unsupported,
    Corrupt,
    Unreadable(IoError),
    Panicked,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Unsupported => formatter.write_str("unsupported format"),
            DecodeError::Corrupt => formatter.write_str("corrupt data"),
            DecodeError::Unreadable(kind) => write!(formatter, "{kind}"),
            DecodeError::Panicked => formatter.write_str("the decoder panicked"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("Cannot decode {}: {kind}", path.display())]
    Decode { path: PathBuf, kind: DecodeError },
    #[error("output device unavailable: {requested}")]
    Device { requested: String },
    #[error("audio output stream: {reason}")]
    Stream { reason: String },
    #[error("Audio output lost: {kind}")]
    OutputLost { kind: StreamError },
    #[error("Cannot preload {}: {kind}", path.display())]
    Preload { path: PathBuf, kind: DecodeError },
    #[error("cannot seek: {reason}")]
    Seek { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    WhileMuted(AudioError),
    WhileNotPlaying(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        domain::{StreamError, ThemeName},
        message::{
            AudioError,
            AudioEvent,
            ConfigEvent,
            DecodeError,
            IoError,
            LibraryError,
            LibraryEvent,
            LibrarySubject,
            MacosEvent,
            Message,
        },
    };

    #[rstest::rstest]
    #[case::audio(Message::from(AudioEvent::Ended), Message::Audio(AudioEvent::Ended))]
    #[case::system(
        Message::from(MacosEvent::OutputRouteChanged),
        Message::Macos(MacosEvent::OutputRouteChanged)
    )]
    #[case::library(
        Message::from(LibraryEvent::Error(LibraryError::NoUserDirs)),
        Message::Library(LibraryEvent::Error(LibraryError::NoUserDirs))
    )]
    #[case::config(
        Message::from(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir"))),
        Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir")))
    )]
    fn a_fact_converts_into_its_message(
        #[case] converted: Message,
        #[case] expected: Message,
    ) {
        assert_eq!(converted, expected);
    }

    #[rstest::rstest]
    #[case::missing_history(
        LibraryError::File {
            subject: LibrarySubject::History,
            path: PathBuf::from("/data/history.jsonl"),
            kind: IoError::Missing,
        },
        "Could not read the history file (/data/history.jsonl): not found"
    )]
    #[case::denied_playlist(
        LibraryError::File {
            subject: LibrarySubject::Playlist,
            path: PathBuf::from("/playlists/My Mix.m3u8"),
            kind: IoError::Denied,
        },
        "Could not read the playlist file (/playlists/My Mix.m3u8): permission denied"
    )]
    #[case::malformed_cache(
        LibraryError::File {
            subject: LibrarySubject::Cache,
            path: PathBuf::from("/data/cache.bin"),
            kind: IoError::Malformed,
        },
        "Could not read the cache (/data/cache.bin): corrupt data"
    )]
    #[case::no_directory(LibraryError::NoUserDirs, "no library directory")]
    fn a_library_failure_renders_its_cause(
        #[case] failure: LibraryError,
        #[case] expected: &str,
    ) {
        assert_eq!(failure.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::decode_unsupported(
        AudioError::Decode {
            path: PathBuf::from("song.flac"),
            kind: DecodeError::Unsupported,
        },
        "Cannot decode song.flac: unsupported format"
    )]
    #[case::preload_panicked(
        AudioError::Preload {
            path: PathBuf::from("song.flac"),
            kind: DecodeError::Panicked,
        },
        "Cannot preload song.flac: the decoder panicked"
    )]
    #[case::output_device_gone(
        AudioError::OutputLost { kind: StreamError::DeviceGone },
        "Audio output lost: the device is gone"
    )]
    fn an_audio_failure_renders_its_cause(
        #[case] failure: AudioError,
        #[case] expected: &str,
    ) {
        assert_eq!(failure.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::not_found(std::io::ErrorKind::NotFound, IoError::Missing)]
    #[case::permission_denied(std::io::ErrorKind::PermissionDenied, IoError::Denied)]
    #[case::storage_full(std::io::ErrorKind::StorageFull, IoError::Full)]
    #[case::other(std::io::ErrorKind::Interrupted, IoError::Other)]
    fn io_fault_from_kind(#[case] kind: std::io::ErrorKind, #[case] expected: IoError) {
        assert_eq!(IoError::from(kind), expected);
    }
}
