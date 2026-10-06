use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    domain::{
        appearance::AppearancePatch,
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
        settings::ReplayGain,
        sleep_presets::SleepPresets,
        speed::Speed,
        theme::{ThemeChoice, ThemeName},
        track::{Track, TrackSource},
    },
    message::{Message, Timer},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScanMode {
    #[default]
    Fresh,
    Cached,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigPatch {
    pub crossfade: Option<Crossfade>,
    pub device: Option<OutputDevice>,
    pub replay_gain: Option<ReplayGain>,
    pub theme_name: Option<ThemeName>,
    pub volume: Option<Percent>,
    pub sleep_presets: Option<SleepPresets>,
    pub music_dir: Option<PathBuf>,
}

impl ConfigPatch {
    #[must_use]
    pub fn then(self, later: Self) -> Self {
        Self {
            crossfade: later.crossfade.or(self.crossfade),
            device: later.device.or(self.device),
            replay_gain: later.replay_gain.or(self.replay_gain),
            theme_name: later.theme_name.or(self.theme_name),
            volume: later.volume.or(self.volume),
            sleep_presets: later.sleep_presets.or(self.sleep_presets),
            music_dir: later.music_dir.or(self.music_dir),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowColorsCmd {
    Set(ThemeName),
    Reset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigCmd {
    Save(ConfigPatch),
    SelectTheme(ThemeChoice),
    SetAppearance(AppearancePatch),
    Flush,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackLoad {
    pub path: PathBuf,
    pub decibels: Option<crate::domain::track::Decibels>,
    pub revision: Revision,
}

impl TrackLoad {
    #[must_use]
    pub fn for_track(track: &Track, revision: Revision) -> Self {
        Self {
            path: track.path().to_path_buf(),
            decibels: track.audio_format().decibels,
            revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub enum DiskCmd {
    AppendHistory(HistoryEntry),
    SaveFavorites(Favorites),
    LoadFavorites,
    Trash(PathBuf),
    LoadHistory(usize),
    SavePlaylist {
        name: PlaylistFileName,
        tracks: Vec<Arc<Track>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryCmd {
    Disk(DiskCmd),
    Scan {
        music_dir: PathBuf,
        revision: Revision,
        mode: ScanMode,
    },
    TagTracks {
        music_dir: PathBuf,
        track_sources: Vec<TrackSource>,
        revision: Revision,
    },
    DecodeCover(CoverJob),
    PrefetchCover(CoverJob),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverJob {
    pub path: PathBuf,
    pub side: Pixels,
}

#[derive(Debug, Clone, PartialEq)]
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

    pub fn then(mut self, cmd: Cmd<E, M>) -> Cmd<E, M> {
        self.effects.extend(cmd.effects);
        self.messages.extend(cmd.messages);
        self
    }

    pub fn map_effect<F>(self, lift: impl FnMut(E) -> F) -> Cmd<F, M> {
        Cmd {
            effects: self.effects.into_iter().map(lift).collect(),
            messages: self.messages,
        }
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
    use std::time::Duration;

    use crate::{
        cmd::{AudioCmd, Cmd, ConfigPatch, Effect, Playback},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            theme::ThemeName,
        },
    };

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
    }

    #[test]
    fn config_patch_then_folds_disjoint_fields_and_the_later_field_wins() {
        let earlier_patch = ConfigPatch {
            theme_name: Some(ThemeName::from_static("dark")),
            crossfade: Some(crossfade(1)),
            ..ConfigPatch::default()
        };
        let later = ConfigPatch {
            crossfade: Some(crossfade(3)),
            ..ConfigPatch::default()
        };

        let merged = earlier_patch.then(later);

        assert_eq!(
            merged.theme_name.as_ref().map(ThemeName::as_str),
            Some("dark")
        );
        assert_eq!(merged.crossfade, Some(crossfade(3)));
    }

    #[test]
    fn config_patch_then_an_absent_field_keeps_the_earlier_one() {
        let speakers =
            || OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap());
        let earlier_patch = ConfigPatch {
            device: Some(speakers()),
            ..ConfigPatch::default()
        };

        let merged = earlier_patch.then(ConfigPatch {
            ..ConfigPatch::default()
        });

        assert_eq!(merged.device, Some(speakers()));
    }

    #[test]
    fn then_with_none_keeps_the_other_cmd() {
        let cmd: Cmd = Cmd::effect(Effect::Audio(AudioCmd::Stop));
        assert_eq!(Cmd::none().then(cmd.clone()), cmd);
        assert_eq!(cmd.clone().then(Cmd::none()), cmd);
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
    fn map_effect_lifts_each_effect_and_keeps_the_messages() {
        let cmd: Cmd<u8, &str> = Cmd::effect(1).then(Cmd::message("a"));
        assert_eq!(
            cmd.map_effect(Some).into_parts(),
            (vec![Some(1)], vec!["a"])
        );
    }

    #[test]
    fn a_cmd_iterates_its_effects_in_order() {
        let cmd: Cmd = Cmd::from_iter([
            Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
            Effect::Audio(AudioCmd::Stop),
        ]);
        {
            let effects: Vec<&Effect> = cmd.effects().collect();
            assert!(matches!(
                effects.as_slice(),
                [
                    Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                    Effect::Audio(AudioCmd::Stop)
                ]
            ));
        }
        let effects: Vec<Effect> = cmd.into_iter().collect();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
    }
}
