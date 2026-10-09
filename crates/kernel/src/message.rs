use std::{path::PathBuf, sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{
    appearance::AppearanceSettings,
    bounded::Bounded,
    chord::ChordPrefix,
    config::{ConfigError, ConfigName, Diagnostic},
    cursor_over::CursorOver,
    device::{DeviceName, ListedDevice, OutputDevice},
    direction::Direction,
    driver::{DriverError, DriverName},
    favorites::{Favorite, Favorites},
    geometry::{Cells, Pixels},
    history::HistoryEntry,
    index::ViewIndex,
    io_error::IoError,
    key::KeyPress,
    keymap::KeymapOverrides,
    overlay::{OverlayName, Verdict},
    percent::Percent,
    playlist::PlaylistFileName,
    revision::Revision,
    server::{
        Connection,
        Fetched,
        Listing,
        Page,
        PlayReport,
        RemoteError,
        ServerName,
        ServerTrackId,
        Session,
    },
    setting_row::SettingRow,
    theme::ThemeName,
    toast::Toast,
    track::{CatalogRow, Track, TrackSource},
    transport::OutputError,
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
    ChordPrefix(ChordPrefix),
    Browse(BrowseRequest),
    Queue(QueueRequest),
    ShuffleRolled(Vec<ViewIndex>),
    Library(LibraryEvent),
    Config(ConfigEvent),
    Audio(AudioEvent),
    Macos(MacosEvent),
    Remote(RemoteEvent),
    Server(ServerRequest),
    Paint(PaintError),
    Elapsed(Timer),
    Driver {
        driver_name: DriverName,
        event: DriverEvent,
    },
    Key(KeyPress),
    Viewport {
        visible_rows: Cells,
        cover_side: Option<Pixels>,
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

impl From<RemoteEvent> for Message {
    fn from(event: RemoteEvent) -> Self {
        Message::Remote(event)
    }
}

impl From<PaintError> for Message {
    fn from(error: PaintError) -> Self {
        Message::Paint(error)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Toast(Revision),
    Sleep(Revision),
    Lookahead(Revision),
    Fetch(Revision),
    Scrobble(Revision),
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
    Settings(SettingRowRequest),
    Text(TextRequest),
    History(HistoryRequest),
    Navigate(Direction),
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SearchRequest {
    Edit(TextRequest),
    Navigate(Direction),
    Enqueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRowRequest {
    Navigate(Direction),
    Step(Direction),
    Activate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRequest {
    Char(char),
    Backspace,
    DeleteWord,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    Navigate(Direction),
    SelectFirst,
    SelectLast,
    Enqueue,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigEvent {
    KeymapReloaded(Box<KeymapOverrides>),
    ThemeReloaded(ThemeName),
    AppearanceReloaded(AppearanceSettings),
    ThemesLoaded {
        theme_names: Vec<ThemeName>,
        refused: Vec<String>,
    },
    MusicDirReloaded(PathBuf),
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
    JumpTo(ViewIndex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeekTenths(u8);

pub(crate) const SEEK_TENTHS_MAX: u8 = 9;

impl SeekTenths {
    #[must_use]
    pub(crate) fn get(self) -> u8 {
        self.0
    }
}

impl Bounded for SeekTenths {
    type Raw = u8;

    const MIN: u8 = 0;
    const MAX: u8 = SEEK_TENTHS_MAX;

    fn within_bounds(raw: u8) -> Self {
        Self(raw)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowseRequest {
    Trash(TrackSource),
    SavePlaylist(PlaylistFileName),
    CursorBy { rows: isize },
    SelectFirst,
    SelectLast,
    PlaySelected,
    CycleSort,
    Rescan,
    ToggleFavorite,
    PageBy(Direction),
    StepCatalog(Direction),
    LevelUp,
    Open(CursorOver<Vec<CatalogRow>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum QueueRequest {
    Toggle,
    ToggleAt(ViewIndex),
    ToggleHistoryEntry(usize),
    PlayNext,
    Dequeue,
    Move(Direction),
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
    Trashed(PathBuf),
    Checked {
        verdict: Verdict,
        revision: Revision,
    },
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

impl LibrarySubject {
    fn operation(self) -> &'static str {
        match self {
            LibrarySubject::Scan | LibrarySubject::Watch => "read",
            LibrarySubject::Trash => "write to",
            LibrarySubject::Playlist
            | LibrarySubject::History
            | LibrarySubject::Favorites
            | LibrarySubject::Cache => "read or write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryError {
    #[error("Could not {} {subject} ({}): {error}", subject.operation(), path.display())]
    Disk {
        subject: LibrarySubject,
        path: PathBuf,
        error: IoError,
    },
    #[error("Could not read the cover of {}: {diagnostic}", path.display())]
    DecodeCover {
        path: PathBuf,
        diagnostic: Diagnostic,
    },
    #[error("no library directory")]
    NoUserDirs,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioEvent {
    PositionReported {
        position: Duration,
        revision: Revision,
    },
    TrackChanged,
    Ended,
    Loaded(Option<Duration>),
    Error(AudioError),
    OutputLost(OutputError),
    DevicesListed(Vec<ListedDevice>),
    DeviceFellBack(OutputDevice),
    DeviceOpened(DeviceName),
    Buffering(Revision),
    Buffered(Revision),
    PreloadCancelled(Revision),
    PreloadKept(Revision),
}

#[derive(Debug, Clone, Copy, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MacosEvent {
    VolumeChanged(Percent),
    OutputRouteChanged,
    Error(MacosError),
    MediaKeyPressed(PlaybackRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteEvent {
    Connected {
        server_name: ServerName,
        session: Session,
    },
    Listed(CatalogPage),
    Error(RemoteError),
    Fetched {
        revision: Revision,
        result: Result<Fetched, RemoteError>,
    },
    Found {
        server_name: ServerName,
        result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
        revision: Revision,
    },
    Starred(ServerFavorite),
    Restored(Result<Vec<PlayReport>, IoError>),
    Unsaved(IoError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPage {
    pub server_name: ServerName,
    pub listing: Listing,
    pub page: Page,
    pub catalog_rows: Vec<CatalogRow>,
    pub favorites: Favorites,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFavorite {
    pub server_name: ServerName,
    pub server_track_id: ServerTrackId,
    pub favorite: Favorite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRequest {
    Add {
        connection: Connection,
        origin_server_name: Option<ServerName>,
    },
    Reconnect(ServerName),
    Remove(ServerName),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaintError {
    #[error("Window colors failed")]
    WriteWindowColors(Diagnostic),
    #[error("Terminal probe failed")]
    Query(Diagnostic),
}

impl PaintError {
    #[must_use]
    pub(crate) fn diagnostic(&self) -> &Diagnostic {
        match self {
            Self::WriteWindowColors(diagnostic) | Self::Query(diagnostic) => diagnostic,
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
    Listen(OsStatus),
    #[error("Cannot follow the new audio device (CoreAudio status {0})")]
    Rebind(OsStatus),
    #[error("Cannot set the system volume (CoreAudio status {0})")]
    SetVolume(OsStatus),
    #[error("Cannot read the cover file: {0}")]
    ReadArtwork(IoError),
    #[error("Cannot open the Files and Folders settings: {0}")]
    Privacy(IoError),
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
    #[error("Cannot decode {}: {error}", path.display())]
    Decode { path: PathBuf, error: DecodeError },
    #[error("output device unavailable: {requested_device} ({diagnostic})")]
    OpenDevice {
        requested_device: OutputDevice,
        diagnostic: Diagnostic,
    },
    #[error("cannot list output devices: {diagnostic}")]
    ListDevices { diagnostic: Diagnostic },
    #[error("Cannot preload {}: {error}", path.display())]
    Preload { path: PathBuf, error: DecodeError },
    #[error("cannot seek: {diagnostic}")]
    Seek { diagnostic: Diagnostic },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        domain::{
            config::Diagnostic,
            device::OutputDevice,
            io_error::IoError,
            theme::ThemeName,
        },
        message::{
            AudioError,
            AudioEvent,
            ConfigEvent,
            DecodeError,
            LibraryError,
            LibraryEvent,
            LibrarySubject,
            MacosError,
            MacosEvent,
            Message,
            OsStatus,
            PaintError,
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
        #[case] message: Message,
        #[case] expected: Message,
    ) {
        assert_eq!(message, expected);
    }

    #[rstest::rstest]
    #[case::missing_history(
        LibraryError::Disk {
            subject: LibrarySubject::History,
            path: PathBuf::from("/data/history.jsonl"),
            error: IoError::Missing,
        },
        "Could not read or write the history file (/data/history.jsonl): not found"
    )]
    #[case::denied_playlist(
        LibraryError::Disk {
            subject: LibrarySubject::Playlist,
            path: PathBuf::from("/playlists/My Mix.m3u8"),
            error: IoError::Denied,
        },
        "Could not read or write the playlist file (/playlists/My Mix.m3u8): permission denied"
    )]
    #[case::malformed_cache(
        LibraryError::Disk {
            subject: LibrarySubject::Cache,
            path: PathBuf::from("/data/cache.bin"),
            error: IoError::Malformed,
        },
        "Could not read or write the cache (/data/cache.bin): corrupt data"
    )]
    #[case::no_directory(LibraryError::NoUserDirs, "no library directory")]
    fn a_library_failure_renders_its_cause(
        #[case] error: LibraryError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn a_write_side_disk_error_says_it_could_not_write() {
        let error = LibraryError::Disk {
            subject: LibrarySubject::Trash,
            path: PathBuf::from("/music/gone.flac"),
            error: IoError::Denied,
        };
        assert_eq!(
            error.to_string(),
            "Could not write to the trash (/music/gone.flac): permission denied"
        );
    }

    #[rstest::rstest]
    #[case::decode_unsupported(
        AudioError::Decode {
            path: PathBuf::from("song.flac"),
            error: DecodeError::Unsupported,
        },
        "Cannot decode song.flac: unsupported format"
    )]
    #[case::preload_panicked(
        AudioError::Preload {
            path: PathBuf::from("song.flac"),
            error: DecodeError::Panicked,
        },
        "Cannot preload song.flac: the decoder panicked"
    )]
    #[case::open_device(
        AudioError::OpenDevice {
            requested_device: OutputDevice::SystemDefault,
            diagnostic: Diagnostic::from_error(&IoError::Missing),
        },
        "output device unavailable: default (not found)"
    )]
    fn an_audio_failure_renders_its_cause(
        #[case] error: AudioError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::hardware_watch(
        MacosError::Listen(OsStatus(-50)),
        "Audio device watch failed (CoreAudio status -50)"
    )]
    #[case::rebind(
        MacosError::Rebind(OsStatus(560_227_702)),
        "Cannot follow the new audio device (CoreAudio status 560227702)"
    )]
    #[case::volume(
        MacosError::SetVolume(OsStatus(0)),
        "Cannot set the system volume (CoreAudio status 0)"
    )]
    #[case::cover(
        MacosError::ReadArtwork(IoError::Denied),
        "Cannot read the cover file: permission denied"
    )]
    fn a_macos_failure_renders_its_cause(
        #[case] macos_error: MacosError,
        #[case] expected: &str,
    ) {
        assert_eq!(macos_error.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::negative(OsStatus(-10_851), "-10851")]
    #[case::zero(OsStatus(0), "0")]
    #[case::positive(OsStatus(1_852_797_029), "1852797029")]
    fn an_os_status_renders_its_code(#[case] status: OsStatus, #[case] expected: &str) {
        assert_eq!(status.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::unsupported(DecodeError::Unsupported, "unsupported format")]
    #[case::corrupt(DecodeError::Corrupt, "corrupt data")]
    #[case::unreadable(DecodeError::Unreadable(IoError::Missing), "not found")]
    #[case::panicked(DecodeError::Panicked, "the decoder panicked")]
    fn a_decode_error_renders_its_cause(
        #[case] decode_error: DecodeError,
        #[case] expected: &str,
    ) {
        assert_eq!(decode_error.to_string(), expected);
    }

    #[rstest::rstest]
    #[case::window_colors(
        PaintError::WriteWindowColors(Diagnostic::from_error(&IoError::Full)),
        "Window colors failed",
        "disk full"
    )]
    #[case::query(
        PaintError::Query(Diagnostic::from_error(&IoError::Other)),
        "Terminal probe failed",
        "an unknown error"
    )]
    fn a_paint_failure_renders_its_title_and_keeps_its_diagnostic(
        #[case] paint_error: PaintError,
        #[case] title: &str,
        #[case] diagnostic: &str,
    ) {
        assert_eq!(
            (paint_error.to_string(), paint_error.diagnostic().text()),
            (title.to_owned(), diagnostic)
        );
    }
}
