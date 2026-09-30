#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

pub mod cmd;
pub mod domain;
pub mod message;
pub mod outbox;
pub mod search;
pub mod update;

pub use cmd::{
    AudioCmd,
    Cmd,
    ConfigCmd,
    ConfigPatch,
    Cue,
    DevicePatch,
    Effect,
    LibraryCmd,
    MacosCmd,
    NowPlaying,
    Playback,
    PlaybackChange,
    WindowColorsCmd,
};
pub use domain::{
    AbLoop,
    AudioFormat,
    Bounded,
    CursorOver,
    Direction,
    Favorites,
    HistoryEntry,
    Key,
    KeyCode,
    KeyPress,
    Model,
    Modifiers,
    Moment,
    Overlay,
    OverlayName,
    Pause,
    Percent,
    Player,
    Playhead,
    Preload,
    SearchQuery,
    SleepTimer,
    Speed,
    TOAST_LIFETIME,
    Tagging,
    Tags,
    Toast,
    ToastLevel,
    Track,
    Transport,
    UnixSeconds,
    Workspace,
    library,
    playlist,
};
pub use message::{
    AudioError,
    AudioEvent,
    BrowseRequest,
    ConfigEvent,
    DecodeError,
    DriverMessage,
    EngineError,
    Gesture,
    HistoryRequest,
    IoError,
    LibraryError,
    LibraryEvent,
    LibrarySubject,
    MacosEvent,
    Message,
    OverlayRequest,
    PlaybackRequest,
    PlaylistRequest,
    QueueRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    TextRequest,
    Timer,
    WorkspaceRequest,
};
pub use outbox::{Outbox, Refusals, SendError};
pub use playlist::Playlist;
pub use update::{keymap::route, startup};
