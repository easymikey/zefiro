use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{
    ChordPrefix,
    ConfigFailure,
    ConfigSource,
    CustomSetting,
    Driver,
    DriverFailure,
    HistoryEntry,
    KeymapOverrides,
    Nudge,
    OutputDevice,
    OverlayName,
    Percent,
    PlaylistIndex,
    Revision,
    SettingRow,
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
    Audio(AudioEvent),
    SystemVolume(Percent),
    Elapsed(Timer),
    Driver(Driver, DriverMessage),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Timer {
    Toast(Revision),
    Sleep(Revision),
}

#[derive(Debug, Clone, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DriverMessage {
    Died(DriverFailure),
    Stopped,
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
    KeymapReloaded(Box<KeymapOverrides>),
    ThemeReloaded,
    SourceFailed { source: ConfigSource, text: String },
    SourceRecovered(ConfigSource),
    ConfigFailed(ConfigFailure),
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
    PageBy(usize, Nudge),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LoadedRequest {
    Jump(PlaylistIndex),
    ShuffleRolled(Vec<usize>),
    FavoritesLoaded(HashSet<PathBuf>),
    LibraryLoaded {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    LibraryListed {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    TracksTagged {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
    },
    HistoryLoaded(Vec<HistoryEntry>),
    ThemesLoaded(Vec<String>),
    MusicDirReloaded(PathBuf),
    CustomRowsReloaded(Vec<CustomSetting>),
    Failed(LibraryFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryFailure {
    #[error("{reason}")]
    Scan { reason: String },
    #[error("{reason}")]
    Playlist { reason: String },
    #[error("{reason}")]
    History { reason: String },
    #[error("{reason}")]
    Favorites { reason: String },
    #[error("{reason}")]
    Trash { reason: String },
    #[error("{reason}")]
    Cache { reason: String },
    #[error("{reason}")]
    Watch { reason: String },
    #[error("no library directory")]
    NoDirectory,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioEvent {
    Position(Duration),
    TrackChanged,
    Ended,
    Loaded { total: Option<Duration> },
    Error(AudioFailure),
    Rejected(EngineRejection),
    DevicesLoaded(Vec<OutputDevice>),
    DeviceFellBack(Option<String>),
    OutputRouteChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioFailure {
    #[error("cannot decode {path}: {reason}")]
    Decode { path: PathBuf, reason: String },
    #[error("output device unavailable: {requested}")]
    Device { requested: String },
    #[error("audio output stream: {reason}")]
    Stream { reason: String },
    #[error("audio output stopped: {reason}")]
    OutputLost { reason: String },
    #[error("preload {path}: {reason}")]
    Preload { path: PathBuf, reason: String },
    #[error("cannot seek: {reason}")]
    Seek { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRejection {
    WhileMuted(AudioFailure),
    WhileNotPlaying(PathBuf),
}
