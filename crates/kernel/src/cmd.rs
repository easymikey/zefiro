use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use strum::IntoStaticStr;

use crate::{
    domain::{
        crossfade::Crossfade,
        cue::{Cue, PlaybackChange},
        device::OutputDevice,
        driver::DriverName,
        favorites::Favorites,
        geometry::Pixels,
        history::HistoryEntry,
        percent::Percent,
        playlist::PlaylistFileName,
        revision::Revision,
        setting_row::{AppearanceField, OptionIndex},
        settings::ReplayGain,
        sleep_presets::SleepPresets,
        speed::Speed,
        theme::{ThemeChoice, ThemeName},
        track::{Track, TrackRef},
    },
    message::{Message, Timer},
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
    Set(ThemeName),
    Reset,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigCmd {
    Save(ConfigPatch),
    SelectTheme(ThemeChoice),
    SetAppearance {
        field: AppearanceField,
        option: OptionIndex,
    },
    Flush,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackLoad {
    pub path: PathBuf,
    pub gain: Option<crate::domain::track::Decibels>,
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
    SetPlayback(Playback),
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
    LoadHistory(usize),
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
        tracks: Vec<TrackRef>,
        revision: Revision,
    },
    DecodeCover(CoverJob),
    PrefetchCover(CoverJob),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverJob {
    pub path: PathBuf,
    pub side: Pixels,
}

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MacosCmd {
    NowPlaying(Option<Arc<Track>>),
    SetPlayback(Playback),
    SetPosition(Duration),
    SetVolume(Percent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
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
    RollShuffle(usize),
    After { delay: Duration, timer: Timer },
    Restart(DriverName),
    Quit,
}

impl PlaybackChange {
    pub fn effects(self) -> [Effect; 2] {
        let (audio, playback) = match self {
            PlaybackChange::Play => {
                (AudioCmd::SetPlayback(Playback::Playing), Playback::Playing)
            }
            PlaybackChange::Pause => {
                (AudioCmd::SetPlayback(Playback::Paused), Playback::Paused)
            }
            PlaybackChange::Stop => (AudioCmd::Stop, Playback::Paused),
        };
        [
            Effect::Audio(audio),
            Effect::Macos(MacosCmd::SetPlayback(playback)),
        ]
    }

    pub fn cued(self) -> Cmd {
        self.effects()
            .into_iter()
            .chain([Effect::Animate(Cue::PlaybackChanged(self))])
            .collect()
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub struct Cmd<E = Effect, M = Message> {
    effects: Vec<E>,
    messages: Vec<M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmds<C> {
    pub cmds: Vec<C>,
    pub at: Instant,
}

impl<E, M> Default for Cmd<E, M> {
    fn default() -> Self {
        Cmd::none()
    }
}

impl<E, M> Cmd<E, M> {
    pub fn none() -> Self {
        Cmd {
            effects: Vec::new(),
            messages: Vec::new(),
        }
    }

    pub fn effect(effect: E) -> Self {
        Cmd {
            effects: vec![effect],
            messages: Vec::new(),
        }
    }

    pub fn message(message: M) -> Self {
        Cmd {
            effects: Vec::new(),
            messages: vec![message],
        }
    }

    pub fn effects(&self) -> std::slice::Iter<'_, E> {
        self.effects.iter()
    }

    #[must_use]
    pub fn into_parts(self) -> (Vec<E>, Vec<M>) {
        (self.effects, self.messages)
    }

    pub fn then(mut self, other: Cmd<E, M>) -> Cmd<E, M> {
        self.effects.extend(other.effects);
        self.messages.extend(other.messages);
        self
    }
}

impl<E, M> IntoIterator for Cmd<E, M> {
    type Item = E;
    type IntoIter = std::vec::IntoIter<E>;

    fn into_iter(self) -> Self::IntoIter {
        self.effects.into_iter()
    }
}

impl From<Effect> for Cmd {
    fn from(effect: Effect) -> Self {
        Cmd::effect(effect)
    }
}

impl From<Cue> for Cmd {
    fn from(cue: Cue) -> Self {
        Cmd::effect(Effect::Animate(cue))
    }
}

impl<E, M> FromIterator<E> for Cmd<E, M> {
    fn from_iter<I: IntoIterator<Item = E>>(effects: I) -> Self {
        Cmd {
            effects: effects.into_iter().collect(),
            messages: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::{AudioCmd, Cmd, Effect, Playback};

    #[test]
    fn then_with_none_keeps_the_other_cmd() {
        let leading: Cmd = Cmd::effect(Effect::Audio(AudioCmd::Stop));
        assert_eq!(Cmd::none().then(leading.clone()), leading);
        let trailing: Cmd = Cmd::effect(Effect::Audio(AudioCmd::Stop));
        assert_eq!(trailing.clone().then(Cmd::none()), trailing);
    }

    #[test]
    fn then_appends_effects_in_order() {
        let first: Cmd =
            Cmd::effect(Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)));
        let second = Cmd::effect(Effect::Audio(AudioCmd::Stop));
        let merged = first.then(second);
        assert_eq!(
            merged,
            Cmd::<Effect>::from_iter([
                Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop),
            ])
        );
    }

    #[test]
    fn a_cmd_iterates_its_effects_in_order() {
        let cmd: Cmd = Cmd::from_iter([
            Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
            Effect::Audio(AudioCmd::Stop),
        ]);
        let borrowed: Vec<&Effect> = cmd.effects().collect();
        assert!(matches!(
            borrowed.as_slice(),
            [
                Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
        let owned: Vec<Effect> = cmd.into_iter().collect();
        assert!(matches!(
            owned.as_slice(),
            [
                Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
    }
}
