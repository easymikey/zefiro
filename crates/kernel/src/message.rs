use std::{path::PathBuf, sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{
    AppearanceSetting,
    ChordPrefix,
    ConfigError,
    ConfigName,
    Diagnostic,
    Direction,
    DriverError,
    DriverName,
    Favorites,
    HistoryEntry,
    IoError,
    KeyPress,
    KeymapOverrides,
    ListedDevice,
    OutputDevice,
    OverlayName,
    Percent,
    Revision,
    SettingRow,
    StreamError,
    ThemeName,
    Toast,
    Track,
    TrackIndex,
    TrackRef,
    ViewIndex,
    appearance::Appearance,
    playlist::PlaylistFileName,
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Message {
    Overlay(OverlayRequest),
    Step {
        row: SettingRow,
        direction: Direction,
    },
    Toast(Toast),
    Playback(PlaybackRequest),
    Browse(BrowseRequest),
    Queue(QueueRequest),
    Playlist(PlaylistRequest),
    ShuffleRolled(Vec<TrackIndex>),
    Library(LibraryEvent),
    Config(ConfigEvent),
    Audio(AudioEvent),
    Macos(MacosEvent),
    Paint(PaintEvent),
    Elapsed(Timer),
    Driver {
        driver: DriverName,
        event: DriverEvent,
    },
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

impl From<PaintEvent> for Message {
    fn from(event: PaintEvent) -> Self {
        Message::Paint(event)
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
    Lookahead(Revision),
}

#[derive(Debug, Clone, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DriverEvent {
    Died(DriverError),
    Stopped,
    Full,
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
    Step(Direction),
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

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigEvent {
    KeymapReloaded(Box<KeymapOverrides>),
    ThemeReloaded(ThemeName),
    AppearanceReloaded(Appearance),
    ThemesLoaded(Vec<ThemeName>),
    MusicDirReloaded(PathBuf),
    AppearanceSettingsReloaded(Vec<AppearanceSetting>),
    Reloaded(ConfigReload),
    Error(ConfigError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigReload {
    pub name: ConfigName,
    pub result: Result<(), ConfigError>,
}

#[derive(Debug, Clone, Copy, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlaybackRequest {
    Toggle,
    Play,
    Pause,
    SeekForward,
    SeekBack,
    HoldForOverlay,
    Release,
    Stop,
    Next,
    Previous,
    SeekBy { direction: Direction, by: Duration },
    StepVolume(Direction),
    ToggleShuffle,
    CycleRepeat,
    CycleSleep,
    AbMark,
    StepSpeed(Direction),
    SeekTo(Duration),
    SeekTenths(SeekTenths),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeekTenths(u8);

pub const SEEK_TENTHS_MAX: u8 = 9;

impl SeekTenths {
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SeekTenthsError {
    #[error("seek tenths {value} is above {max}")]
    OutOfRange { value: u8, max: u8 },
}

impl TryFrom<u8> for SeekTenths {
    type Error = SeekTenthsError;

    fn try_from(tenths: u8) -> Result<Self, Self::Error> {
        if tenths <= SEEK_TENTHS_MAX {
            Ok(Self(tenths))
        } else {
            Err(SeekTenthsError::OutOfRange {
                value: tenths,
                max: SEEK_TENTHS_MAX,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseRequest {
    Trash(TrackRef),
    SavePlaylist(PlaylistFileName),
    ChordPrefix(ChordPrefix),
    CursorBy { rows: isize },
    Top,
    Bottom,
    PlaySelected,
    CycleSort,
    FullScan,
    ToggleFavorite,
    CursorTo(ViewIndex),
    PageBy(Direction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum QueueRequest {
    Enqueue,
    EnqueueTrack(ViewIndex),
    PlayNext,
    Dequeue,
    MoveInQueue(Direction),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlaylistRequest {
    JumpTo(ViewIndex),
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
    FavoritesLoaded(Favorites),
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
    Loaded(Option<Duration>),
    Error(AudioError),
    DevicesListed(Vec<ListedDevice>),
    DeviceFellBack(OutputDevice),
}

#[derive(Debug, Clone, Copy, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MacosEvent {
    Volume(Percent),
    OutputRouteChanged,
    Error(MacosError),
    MediaKey(PlaybackRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PaintEvent {
    Error(PaintError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaintError {
    #[error("Window colors failed")]
    WindowColors(Diagnostic),
    #[error("Cover art failed")]
    Cover(Diagnostic),
    #[error("Terminal probe failed")]
    Probe(Diagnostic),
}

impl PaintError {
    #[must_use]
    pub fn diagnostic(&self) -> &Diagnostic {
        match self {
            Self::WindowColors(diagnostic)
            | Self::Cover(diagnostic)
            | Self::Probe(diagnostic) => diagnostic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported format")]
    Unsupported,
    #[error("corrupt data")]
    Corrupt,
    #[error("{0}")]
    Unreadable(IoError),
    #[error("the decoder panicked")]
    Panicked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MacosError {
    #[error("Audio device watch failed (CoreAudio status {0})")]
    HardwareWatch(OsStatus),
    #[error("Cannot follow the new audio device (CoreAudio status {0})")]
    Rebind(OsStatus),
    #[error("Cannot set the system volume (CoreAudio status {0})")]
    Volume(OsStatus),
    #[error("Cannot read the cover file: {0}")]
    Cover(std::io::ErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsStatus(pub i32);

impl std::fmt::Display for OsStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("Cannot decode {}: {kind}", path.display())]
    Decode { path: PathBuf, kind: DecodeError },
    #[error("output device unavailable: {requested}")]
    Device { requested: OutputDevice },
    #[error("cannot list output devices: {reason}")]
    ListDevices { reason: Diagnostic },
    #[error("audio output stream: {reason}")]
    Stream { reason: Diagnostic },
    #[error("Audio output lost: {0}")]
    OutputLost(StreamError),
    #[error("Cannot preload {}: {kind}", path.display())]
    Preload { path: PathBuf, kind: DecodeError },
    #[error("cannot seek: {reason}")]
    Seek { reason: Diagnostic },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        domain::{IoError, StreamError, ThemeName},
        message::{
            AudioError,
            AudioEvent,
            ConfigEvent,
            DecodeError,
            LibraryError,
            LibraryEvent,
            LibrarySubject,
            MacosEvent,
            Message,
        },
    };

    #[rstest::rstest]
    #[case::audio(Message::from(AudioEvent::Ended), Message::Audio(AudioEvent::Ended))]
    #[case::macos(
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
    fn an_event_converts_into_its_message(
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
        AudioError::OutputLost(StreamError::DeviceGone),
        "Audio output lost: the device is gone"
    )]
    fn an_audio_failure_renders_its_cause(
        #[case] failure: AudioError,
        #[case] expected: &str,
    ) {
        assert_eq!(failure.to_string(), expected);
    }
}
