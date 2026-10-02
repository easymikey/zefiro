use std::{path::PathBuf, sync::Arc, time::Duration};

use strum::{EnumIter, IntoStaticStr};

use crate::{
    domain::{
        Crossfade,
        Driver,
        Favorites,
        HistoryEntry,
        OptionIndex,
        OutputDevice,
        Percent,
        ReplayGain,
        Revision,
        SleepPresets,
        Speed,
        ThemeChoice,
        ThemeName,
        Track,
        appearance_rows::AppearanceField,
        playlist::PlaylistFileName,
    },
    message::Timer,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScanMode {
    #[default]
    Full,
    Cached,
}

#[derive(Debug, Clone, PartialEq, bon::Builder)]
pub struct ConfigPatch {
    #[builder(setters(option_fn(name = with_crossfade)))]
    pub crossfade: Option<Crossfade>,
    #[builder(setters(option_fn(name = with_device)))]
    pub device: Option<OutputDevice>,
    #[builder(setters(option_fn(name = with_replay_gain)))]
    pub replay_gain: Option<ReplayGain>,
    #[builder(setters(option_fn(name = with_theme)))]
    pub theme: Option<ThemeName>,
    #[builder(setters(option_fn(name = with_volume)))]
    pub volume: Option<Percent>,
    #[builder(setters(option_fn(name = with_sleep_presets)))]
    pub sleep_presets: Option<SleepPresets>,
    #[builder(setters(option_fn(name = with_music_dir)))]
    pub music_dir: Option<PathBuf>,
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
    Setting {
        field: AppearanceField,
        option: OptionIndex,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackLoad {
    pub path: PathBuf,
    pub gain: Option<f32>,
    pub revision: Revision,
}

impl TrackLoad {
    #[must_use]
    pub fn for_track(track: &Track, revision: Revision) -> Self {
        Self {
            path: track.path().to_path_buf(),
            gain: track.audio_format().replay_gain,
            revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AudioCmd {
    Load(TrackLoad),
    Playback(Playback),
    Seek(Duration),
    SetSpeed(Speed),
    Stop,
    Preload(TrackLoad),
    SetCrossfade(Crossfade),
    SetReplayGain(ReplayGain),
    SetDevice(OutputDevice),
    ListDevices,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LibraryCmd {
    AppendHistory(HistoryEntry),
    SaveFavorites(Favorites),
    LoadFavorites,
    Trash(PathBuf),
    LoadHistory {
        limit: usize,
    },
    SavePlaylist {
        name: PlaylistFileName,
        tracks: Vec<Arc<Track>>,
    },
    Scan {
        music_dir: PathBuf,
        revision: Revision,
        mode: ScanMode,
    },
    TagTracks {
        music_dir: PathBuf,
        paths: Vec<PathBuf>,
        revision: Revision,
    },
    PrefetchCover(PathBuf),
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MacosCmd {
    NowPlaying(Option<Arc<Track>>),
    PlaybackState(Playback),
    PlaybackPosition(Duration),
    Volume(Percent),
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
    Macos(MacosCmd),
    Config(ConfigCmd),
    WindowColors(WindowColorsCmd),
    Animate(Cue),
    RollShuffle { len: usize },
    After { delay: Duration, timer: Timer },
    Restart(Driver),
    Quit,
}

impl PlaybackChange {
    pub fn effects(self) -> [Effect; 2] {
        let (audio, playback) = match self {
            PlaybackChange::Play => {
                (AudioCmd::Playback(Playback::Playing), Playback::Playing)
            }
            PlaybackChange::Pause => {
                (AudioCmd::Playback(Playback::Paused), Playback::Paused)
            }
            PlaybackChange::Stop => (AudioCmd::Stop, Playback::Paused),
        };
        [
            Effect::Audio(audio),
            Effect::Macos(MacosCmd::PlaybackState(playback)),
        ]
    }

    pub fn cued(self) -> Cmd {
        Cmd::Batch(
            self.effects()
                .into_iter()
                .chain([Effect::Animate(Cue::PlaybackChanged(self))])
                .collect(),
        )
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
    fn then_with_none_keeps_the_other_cmd() {
        let leading = Cmd::One(Effect::Audio(AudioCmd::Stop));
        assert!(matches!(Cmd::None.then(leading), Cmd::One(_)));
        let trailing = Cmd::One(Effect::Audio(AudioCmd::Stop));
        assert!(matches!(trailing.then(Cmd::None), Cmd::One(_)));
    }

    #[test]
    fn then_merges_two_single_effects_into_a_batch() {
        let first = Cmd::One(Effect::Audio(AudioCmd::Playback(Playback::Paused)));
        let second = Cmd::One(Effect::Audio(AudioCmd::Stop));
        let merged = first.then(second);
        assert!(matches!(merged, Cmd::Batch(effects) if effects.len() == 2));
    }

    #[test]
    fn a_batch_iterates_its_effects_in_order() {
        let cmd = Cmd::Batch(vec![
            Effect::Audio(AudioCmd::Playback(Playback::Paused)),
            Effect::Audio(AudioCmd::Stop),
        ]);
        let effects: Vec<Effect> = cmd.into_iter().collect();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Audio(AudioCmd::Playback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
    }
}
