use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use strum::{EnumIter, IntoStaticStr};

use crate::{
    domain::{
        Crossfade,
        DeviceName,
        Driver,
        OptionIndex,
        Percent,
        Replaygain,
        Revision,
        SettingId,
        Speed,
        ThemeChoice,
        ThemeName,
        Track,
        UnixSeconds,
        playlist::PlaylistFileName,
    },
    message::Timer,
};

#[derive(Debug, Clone, PartialEq)]
pub enum DevicePatch {
    Keep,
    SystemDefault,
    Named(DeviceName),
}

#[derive(Debug, Clone, PartialEq, bon::Builder)]
#[builder(on(String, into))]
pub struct ConfigPatch {
    #[builder(setters(option_fn(name = with_crossfade)))]
    pub crossfade: Option<Crossfade>,
    #[builder(default = DevicePatch::Keep, setters(option_fn(name = with_device)))]
    pub device: DevicePatch,
    #[builder(setters(option_fn(name = with_replaygain)))]
    pub replaygain: Option<Replaygain>,
    #[builder(setters(option_fn(name = with_theme)))]
    pub theme: Option<ThemeName>,
    #[builder(setters(option_fn(name = with_volume)))]
    pub volume: Option<Percent>,
    #[builder(setters(option_fn(name = with_sleep_presets)))]
    pub sleep_presets: Option<Vec<Duration>>,
    #[builder(setters(option_fn(name = with_music_dir)))]
    pub music_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowColorsCmd {
    Apply(ThemeName),
    Reset,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigCmd {
    Save(ConfigPatch),
    SelectTheme(ThemeChoice),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioCmd {
    Load {
        path: PathBuf,
        gain: Option<f32>,
        revision: Revision,
    },
    Pause(Playback),
    Seek(Duration),
    SetSpeed(Speed),
    Stop,
    Preload {
        path: PathBuf,
        gain: Option<f32>,
        revision: Revision,
    },
    SetCrossfade(Crossfade),
    SetReplaygain(Replaygain),
    SetDevice(Option<DeviceName>),
    ListDevices,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LibraryCmd {
    AppendHistory {
        track: Arc<Track>,
        at: UnixSeconds,
    },
    SaveFavorites(Arc<HashSet<PathBuf>>),
    LoadFavorites,
    Trash(PathBuf),
    LoadHistory {
        limit: usize,
    },
    Rescan {
        root: PathBuf,
        revision: Revision,
    },
    SavePlaylist {
        name: PlaylistFileName,
        tracks: Vec<Arc<Track>>,
    },
    ScanLibrary {
        root: PathBuf,
        revision: Revision,
    },
    TagTracks {
        root: PathBuf,
        paths: Vec<PathBuf>,
        revision: Revision,
    },
    PrefetchCover(PathBuf),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SystemCmd {
    NowPlaying(NowPlaying),
    PlaybackState(Playback),
    PlaybackPosition(Duration),
    Volume(Percent),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum NowPlaying {
    #[default]
    Cleared,
    Track {
        title: String,
        artist: Option<String>,
        album: Option<String>,
        duration: Duration,
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
}

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

#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub enum Effect {
    Audio(AudioCmd),
    Library(LibraryCmd),
    System(SystemCmd),
    Config(ConfigCmd),
    WindowColors(WindowColorsCmd),
    Animate(Cue),
    RollShuffle { len: usize },
    Setting { id: SettingId, option: OptionIndex },
    After { delay: Duration, message: Timer },
    Restart(Driver),
    Quit,
}

impl PlaybackChange {
    pub fn effects(self) -> [Effect; 2] {
        let (audio, playback) = match self {
            PlaybackChange::Play => {
                (AudioCmd::Pause(Playback::Playing), Playback::Playing)
            }
            PlaybackChange::Pause => {
                (AudioCmd::Pause(Playback::Paused), Playback::Paused)
            }
            PlaybackChange::Stop => (AudioCmd::Stop, Playback::Paused),
        };
        [
            Effect::Audio(audio),
            Effect::System(SystemCmd::PlaybackState(playback)),
        ]
    }

    pub fn cued(self) -> Cmd {
        let mut effects = self.effects().to_vec();
        effects.push(Effect::Animate(Cue::PlaybackChanged(self)));
        Cmd::Batch(effects)
    }
}

#[must_use]
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Cmd {
    #[default]
    None,
    One(Effect),
    Batch(Vec<Effect>),
}

impl From<Effect> for Cmd {
    fn from(effect: Effect) -> Self {
        Cmd::One(effect)
    }
}

impl From<Cue> for Cmd {
    fn from(cue: Cue) -> Self {
        Cmd::One(Effect::Animate(cue))
    }
}

impl Cmd {
    pub fn effects(&self) -> std::slice::Iter<'_, Effect> {
        match self {
            Cmd::None => [].iter(),
            Cmd::One(effect) => std::slice::from_ref(effect).iter(),
            Cmd::Batch(effects) => effects.iter(),
        }
    }

    pub fn effects_mut(&mut self) -> std::slice::IterMut<'_, Effect> {
        match self {
            Cmd::None => [].iter_mut(),
            Cmd::One(effect) => std::slice::from_mut(effect).iter_mut(),
            Cmd::Batch(effects) => effects.iter_mut(),
        }
    }

    pub fn then(self, other: Cmd) -> Cmd {
        match (self, other) {
            (Cmd::None, other) => other,
            (first, Cmd::None) => first,
            (Cmd::One(first), Cmd::One(second)) => Cmd::Batch(vec![first, second]),
            (Cmd::One(effect), Cmd::Batch(mut rest)) => {
                rest.insert(0, effect);
                Cmd::Batch(rest)
            }
            (Cmd::Batch(mut effects), Cmd::One(effect)) => {
                effects.push(effect);
                Cmd::Batch(effects)
            }
            (Cmd::Batch(mut effects), Cmd::Batch(more)) => {
                effects.extend(more);
                Cmd::Batch(effects)
            }
        }
    }
}

impl IntoIterator for Cmd {
    type Item = Effect;
    type IntoIter = std::vec::IntoIter<Effect>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Cmd::None => Vec::new().into_iter(),
            Cmd::One(effect) => vec![effect].into_iter(),
            Cmd::Batch(effects) => effects.into_iter(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::{AudioCmd, Cmd, Effect, Playback};

    #[test]
    fn then_none_is_identity() {
        let leading = Cmd::One(Effect::Audio(AudioCmd::Stop));
        assert!(matches!(Cmd::None.then(leading), Cmd::One(_)));
        let trailing = Cmd::One(Effect::Audio(AudioCmd::Stop));
        assert!(matches!(trailing.then(Cmd::None), Cmd::One(_)));
    }

    #[test]
    fn then_merges_two_ones_into_batch() {
        let first = Cmd::One(Effect::Audio(AudioCmd::Pause(Playback::Paused)));
        let second = Cmd::One(Effect::Audio(AudioCmd::Stop));
        let merged = first.then(second);
        assert!(matches!(merged, Cmd::Batch(effects) if effects.len() == 2));
    }

    #[test]
    fn into_iter_yields_batch_effects_in_order() {
        let cmd = Cmd::Batch(vec![
            Effect::Audio(AudioCmd::Pause(Playback::Paused)),
            Effect::Audio(AudioCmd::Stop),
        ]);
        let effects: Vec<Effect> = cmd.into_iter().collect();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Audio(AudioCmd::Pause(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
    }
}
