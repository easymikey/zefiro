use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{
    ChordPrefix,
    ConfigFailure,
    ConfigSource,
    CustomSetting,
    DeviceName,
    Driver,
    DriverFailure,
    HistoryEntry,
    KeyPress,
    KeymapOverrides,
    Nudge,
    OutputDevice,
    OutputFault,
    OverlayName,
    Percent,
    PlaylistIndex,
    Revision,
    SettingRow,
    ThemeName,
    Toast,
    Track,
    playlist::PlaylistFileName,
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Message {
    Overlay(OverlayRequest),
    Adjust { row: SettingRow, nudge: Nudge },
    Workspace(WorkspaceRequest),
    Playback(PlaybackRequest),
    Browse(BrowseRequest),
    Loaded(LoadedRequest),
    Library(LibraryFact),
    Config(ConfigFact),
    Audio(AudioEvent),
    System(SystemEvent),
    Elapsed(Timer),
    Driver(Driver, DriverMessage),
    Key(KeyPress),
    Viewport { visible_rows: usize },
    Quit,
}

impl From<AudioEvent> for Message {
    fn from(event: AudioEvent) -> Self {
        Message::Audio(event)
    }
}

impl From<SystemEvent> for Message {
    fn from(event: SystemEvent) -> Self {
        Message::System(event)
    }
}

impl From<LibraryFact> for Message {
    fn from(fact: LibraryFact) -> Self {
        Message::Library(fact)
    }
}

impl From<ConfigFact> for Message {
    fn from(fact: ConfigFact) -> Self {
        Message::Config(fact)
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
    Died(DriverFailure),
    Stopped,
    Congested,
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
    Jump(JumpRequest),
    History(HistoryRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SearchRequest {
    Edit(SearchEdit),
    Navigate(Nudge),
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
    Navigate(Nudge),
    Adjust(Nudge),
    Activate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRequest {
    Char(char),
    Backspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpRequest {
    Char(char),
    Backspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    Navigate(Nudge),
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
pub enum ConfigFact {
    KeymapReloaded(Box<KeymapOverrides>),
    ThemeReloaded(ThemeName),
    ThemesLoaded(Vec<ThemeName>),
    MusicDirReloaded(PathBuf),
    CustomRowsReloaded(Vec<CustomSetting>),
    SourceFailed { source: ConfigSource, text: String },
    SourceRecovered(ConfigSource),
    Failed(ConfigFailure),
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
    Prev,
    SeekBy(i64),
    NudgeVolume(i8),
    ToggleShuffle,
    CycleRepeat,
    CycleSleep,
    AbMark,
    NudgeSpeed(i8),
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
    CursorBy(i64),
    Top,
    Bottom,
    PlaySelected,
    Enqueue,
    EnqueueTrack(PlaylistIndex),
    PlayNext,
    Dequeue,
    MoveInQueue(Nudge),
    CycleSort,
    Rescan,
    ToggleFavorite,
    CursorTo(PlaylistIndex),
    PageBy(Nudge),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LoadedRequest {
    Jump(PlaylistIndex),
    ShuffleRolled(Vec<usize>),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LibraryFact {
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
    Failed(LibraryFailure),
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
pub enum IoFault {
    Missing,
    Denied,
    Malformed,
    Full,
    Other,
}

impl std::fmt::Display for IoFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            IoFault::Missing => "not found",
            IoFault::Denied => "permission denied",
            IoFault::Malformed => "corrupt data",
            IoFault::Full => "disk full",
            IoFault::Other => "an unknown error",
        };
        formatter.write_str(label)
    }
}

impl From<std::io::ErrorKind> for IoFault {
    fn from(kind: std::io::ErrorKind) -> Self {
        if kind == std::io::ErrorKind::NotFound {
            IoFault::Missing
        } else if kind == std::io::ErrorKind::PermissionDenied {
            IoFault::Denied
        } else if kind == std::io::ErrorKind::StorageFull {
            IoFault::Full
        } else {
            IoFault::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryFailure {
    #[error("Could not read {subject} ({}): {fault}", path.display())]
    File {
        subject: LibrarySubject,
        path: PathBuf,
        fault: IoFault,
    },
    #[error("no library directory")]
    NoDirectory,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioEvent {
    Playhead(Duration),
    TrackChanged,
    Ended,
    Loaded { total: Option<Duration> },
    Error(AudioFailure),
    Rejected(EngineRejection),
    DevicesLoaded(Vec<OutputDevice>),
    DeviceFellBack(Option<DeviceName>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SystemEvent {
    Volume(Percent),
    OutputRouteChanged,
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
            Gesture::Previous => PlaybackRequest::Prev,
            Gesture::SeekForward => PlaybackRequest::SeekForward,
            Gesture::SeekBack => PlaybackRequest::SeekBack,
            Gesture::Scrub(position) => PlaybackRequest::SeekTo(position),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeFault {
    Unsupported,
    Corrupt,
    Unreadable(IoFault),
    Panicked,
}

impl std::fmt::Display for DecodeFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeFault::Unsupported => formatter.write_str("unsupported format"),
            DecodeFault::Corrupt => formatter.write_str("corrupt data"),
            DecodeFault::Unreadable(fault) => write!(formatter, "{fault}"),
            DecodeFault::Panicked => formatter.write_str("the decoder panicked"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioFailure {
    #[error("Cannot decode {}: {fault}", path.display())]
    Decode { path: PathBuf, fault: DecodeFault },
    #[error("output device unavailable: {requested}")]
    Device { requested: String },
    #[error("audio output stream: {reason}")]
    Stream { reason: String },
    #[error("Audio output lost: {fault}")]
    OutputLost { fault: OutputFault },
    #[error("Cannot preload {}: {fault}", path.display())]
    Preload { path: PathBuf, fault: DecodeFault },
    #[error("cannot seek: {reason}")]
    Seek { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRejection {
    WhileMuted(AudioFailure),
    WhileNotPlaying(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        domain::{OutputFault, ThemeName},
        message::{
            AudioEvent,
            AudioFailure,
            ConfigFact,
            DecodeFault,
            IoFault,
            LibraryFact,
            LibraryFailure,
            LibrarySubject,
            Message,
            SystemEvent,
        },
    };

    #[rstest::rstest]
    #[case::audio(Message::from(AudioEvent::Ended), Message::Audio(AudioEvent::Ended))]
    #[case::system(
        Message::from(SystemEvent::OutputRouteChanged),
        Message::System(SystemEvent::OutputRouteChanged)
    )]
    #[case::library(
        Message::from(LibraryFact::Failed(LibraryFailure::NoDirectory)),
        Message::Library(LibraryFact::Failed(LibraryFailure::NoDirectory))
    )]
    #[case::config(
        Message::from(ConfigFact::ThemeReloaded(ThemeName::from_static("noir"))),
        Message::Config(ConfigFact::ThemeReloaded(ThemeName::from_static("noir")))
    )]
    fn a_fact_converts_into_its_message(
        #[case] converted: Message,
        #[case] expected: Message,
    ) {
        assert_eq!(converted, expected);
    }

    #[rstest::rstest]
    #[case::missing_history(
        LibraryFailure::File {
            subject: LibrarySubject::History,
            path: PathBuf::from("/data/history.jsonl"),
            fault: IoFault::Missing,
        },
        "Could not read the history file (/data/history.jsonl): not found"
    )]
    #[case::denied_playlist(
        LibraryFailure::File {
            subject: LibrarySubject::Playlist,
            path: PathBuf::from("/playlists/My Mix.m3u8"),
            fault: IoFault::Denied,
        },
        "Could not read the playlist file (/playlists/My Mix.m3u8): permission denied"
    )]
    #[case::malformed_cache(
        LibraryFailure::File {
            subject: LibrarySubject::Cache,
            path: PathBuf::from("/data/cache.bin"),
            fault: IoFault::Malformed,
        },
        "Could not read the cache (/data/cache.bin): corrupt data"
    )]
    #[case::no_directory(LibraryFailure::NoDirectory, "no library directory")]
    fn a_library_failure_renders_its_cause(
        #[case] failure: LibraryFailure,
        #[case] expected: &str,
    ) {
        assert_eq!(failure.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::decode_unsupported(
        AudioFailure::Decode {
            path: PathBuf::from("song.flac"),
            fault: DecodeFault::Unsupported,
        },
        "Cannot decode song.flac: unsupported format"
    )]
    #[case::preload_panicked(
        AudioFailure::Preload {
            path: PathBuf::from("song.flac"),
            fault: DecodeFault::Panicked,
        },
        "Cannot preload song.flac: the decoder panicked"
    )]
    #[case::output_device_gone(
        AudioFailure::OutputLost { fault: OutputFault::DeviceGone },
        "Audio output lost: the device is gone"
    )]
    fn an_audio_failure_renders_its_cause(
        #[case] failure: AudioFailure,
        #[case] expected: &str,
    ) {
        assert_eq!(failure.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::not_found(std::io::ErrorKind::NotFound, IoFault::Missing)]
    #[case::permission_denied(std::io::ErrorKind::PermissionDenied, IoFault::Denied)]
    #[case::storage_full(std::io::ErrorKind::StorageFull, IoFault::Full)]
    #[case::other(std::io::ErrorKind::Interrupted, IoFault::Other)]
    fn io_fault_from_kind(#[case] kind: std::io::ErrorKind, #[case] expected: IoFault) {
        assert_eq!(IoFault::from(kind), expected);
    }
}
