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
    KeyPress,
    Model,
    Modifiers,
    Moment,
    Nudge,
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
    AudioEvent,
    AudioFailure,
    BrowseRequest,
    ConfigFact,
    DecodeFault,
    DriverMessage,
    EngineRejection,
    Gesture,
    HistoryRequest,
    IoFault,
    JumpRequest,
    LibraryFact,
    LibraryFailure,
    LibrarySubject,
    LoadedRequest,
    Message,
    OverlayRequest,
    PlaybackRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    SystemEvent,
    TextRequest,
    Timer,
    WorkspaceRequest,
};
pub use outbox::{Delivery, Outbox};
pub use playlist::Playlist;
pub use update::{keymap::route, startup};
