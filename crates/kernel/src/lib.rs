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
    ConfigCmd,
    ConfigPatch,
    Cue,
    DevicePatch,
    Effect,
    LibraryCmd,
    NowPlaying,
    Playback,
    PlaybackChange,
    SystemCmd,
    WindowColorsCmd,
};
pub use domain::{
    AbLoop,
    AudioFormat,
    Bounded,
    CursorOver,
    Favorites,
    HistoryEntry,
    Key,
    KeyCode,
    Model,
    Modifiers,
    Nudge,
    Overlay,
    OverlayName,
    Pause,
    Percent,
    Player,
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
    Workspace,
    library,
    playlist,
};
pub use message::{
    AudioEvent,
    AudioFailure,
    BrowseRequest,
    DriverMessage,
    EngineRejection,
    HistoryRequest,
    JumpRequest,
    LibraryFailure,
    LoadedRequest,
    Message,
    OverlayRequest,
    PlaybackRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    TextRequest,
    Timer,
    WorkspaceRequest,
};
pub use playlist::Playlist;
pub use update::{keymap::route, startup};
