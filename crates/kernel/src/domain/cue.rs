use strum::{EnumIter, IntoStaticStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlaybackChange {
    #[default]
    Play,
    Pause,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum Cue {
    OverlayOpened,
    OverlayClosed,
    ToastRaised,
    ToastDismissed,
    TrackChanged,
    PlaybackChanged(PlaybackChange),
    QueueChanged,
    FavoriteToggled,
    PlayOrderChanged,
    VolumeChanged,
    TrackDeleted,
    ThemeChanged,
    LibraryOpened,
    LayoutChanged,
}
