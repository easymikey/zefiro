#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

pub mod cmd;
pub mod domain;
pub mod message;
pub mod search;
pub mod update;

pub use cmd::{
    AudioCmd,
    Cmd,
    Cmds,
    ConfigCmd,
    ConfigPatch,
    Cue,
    Effect,
    LibraryCmd,
    MacosCmd,
    Playback,
    PlaybackChange,
    TrackLoad,
    WindowColorsCmd,
};
pub use domain::{
    AbLoop,
    AudioFormat,
    Bounded,
    CursorOver,
    Diagnostic,
    Direction,
    Favorites,
    HistoryEntry,
    IoError,
    Key,
    KeyCode,
    KeyPress,
    Model,
    Modifiers,
    Moment,
    Overlay,
    OverlayName,
    PausedBy,
    Percent,
    Player,
    Playhead,
    Preload,
    SearchQuery,
    SleepTimer,
    Speed,
    TOAST_LIFETIME,
    TOAST_SECONDS,
    TOAST_STACK,
    Tagging,
    Tags,
    Toast,
    ToastKind,
    Track,
    TrackRef,
    Transport,
    Workspace,
    library,
    playlist,
};
pub use message::{
    AudioError,
    AudioEvent,
    BrowseRequest,
    ConfigEvent,
    ConfigReload,
    DecodeError,
    DriverEvent,
    HistoryRequest,
    LibraryError,
    LibraryEvent,
    LibrarySubject,
    MacosError,
    MacosEvent,
    Message,
    OverlayRequest,
    PaintError,
    PaintEvent,
    PlaybackRequest,
    PlaylistRequest,
    QueueRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    TextRequest,
    Timer,
};
pub use playlist::Playlist;
pub use update::{keymap::route, startup};
