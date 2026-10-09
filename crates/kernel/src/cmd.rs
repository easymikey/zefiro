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
        favorites::{Favorite, Favorites},
        geometry::Pixels,
        history::HistoryEntry,
        percent::Percent,
        player::Player,
        playlist::PlaylistFileName,
        revision::Revision,
        server::{
            Account,
            Artwork,
            Connection,
            Download,
            Listing,
            MediaFetch,
            Page,
            PlayReport,
            ServerName,
            ServerTrackId,
            Session,
        },
        settings::ReplayGain,
        sleep_presets::SleepPresets,
        speed::Speed,
        theme::{ThemeChoice, ThemeName},
        track::{Decibels, Track, TrackSource},
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
    pub accounts: Option<Vec<Account>>,
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
            accounts: later.accounts.or(self.accounts),
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
    pub media: Media,
    pub decibels: Option<Decibels>,
    pub revision: Revision,
}

impl TrackLoad {
    #[must_use]
    pub fn for_track(track: &Track, revision: Revision) -> Option<Self> {
        track.local_path().map(|path| Self {
            media: Media::Local(path.to_path_buf()),
            decibels: track.audio_format().decibels,
            revision,
        })
    }

    #[must_use]
    pub fn fetched(track: &Track, download: &Download) -> Option<Self> {
        let fetched = download
            .fetched
            .as_ref()
            .filter(|_fetched| download.ready())?;
        let revision = download.media_fetch.revision;
        let media = if fetched.is_complete() {
            Media::Local(fetched.media_path.clone())
        } else {
            Media::Growing(GrowingMedia {
                media_path: fetched.media_path.clone(),
                downloaded: fetched.downloaded,
                byte_len: fetched.byte_len,
                revision,
            })
        };
        Some(Self {
            media,
            decibels: track.audio_format().decibels,
            revision,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Media {
    Local(PathBuf),
    Growing(GrowingMedia),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowingMedia {
    pub media_path: PathBuf,
    pub downloaded: u64,
    pub byte_len: u64,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioCmd {
    Load(TrackLoad),
    SetPlayback(Playback),
    Seek {
        target: Duration,
        revision: Revision,
    },
    SetSpeed(Speed),
    Stop,
    Preload(TrackLoad),
    CancelPreload(Revision),
    SetCrossfade(Crossfade),
    SetReplayGain(ReplayGain),
    SetDevice(OutputDevice),
    ListDevices,
    Grow {
        revision: Revision,
        downloaded: u64,
    },
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
    Probe {
        path: PathBuf,
        revision: Revision,
    },
    Subfolders {
        path: PathBuf,
        revision: Revision,
    },
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
    SetSpeed(Speed),
    SetVolume(Percent),
    Privacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCmd {
    Connect(Connection),
    Forget(Account),
    List {
        server_name: ServerName,
        session: Session,
        listing: Listing,
        page: Page,
        revision: Revision,
    },
    Fetch(MediaFetch),
    Prefetch(MediaFetch),
    Search {
        server_name: ServerName,
        session: Session,
        input: String,
        listing: Listing,
        revision: Revision,
    },
    Star {
        server_name: ServerName,
        session: Session,
        server_track_id: ServerTrackId,
        favorite: Favorite,
    },
    Report {
        session: Session,
        play_report: PlayReport,
    },
    Flush(Vec<PlayReport>),
    Cover {
        session: Session,
        artwork: Artwork,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
}

impl From<&Player> for Playback {
    fn from(player: &Player) -> Self {
        if player.is_playing() {
            Self::Playing
        } else {
            Self::Paused
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub enum Effect {
    Audio(AudioCmd),
    Library(LibraryCmd),
    Macos(MacosCmd),
    Remote(RemoteCmd),
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

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, ConfigPatch, Effect, Playback},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            server::{Account, Endpoint, ServerName, UserName},
            theme::ThemeName,
        },
    };

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
    }

    fn speakers() -> OutputDevice {
        OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap())
    }

    fn account() -> Account {
        Account {
            server_name: ServerName::new("home"),
            endpoint: Endpoint::parse("https://music.example").unwrap(),
            user_name: UserName::new("ann").unwrap(),
        }
    }

    fn stop() -> Cmd {
        Cmd::effect(Effect::Audio(AudioCmd::Stop))
    }

    fn pause() -> Effect {
        Effect::Audio(AudioCmd::SetPlayback(Playback::Paused))
    }

    #[rstest]
    #[case::disjoint_fields_fold_and_the_later_field_wins(
        ConfigPatch {
            theme_name: Some(ThemeName::from_static("dark")),
            crossfade: Some(crossfade(1)),
            ..ConfigPatch::default()
        },
        ConfigPatch { crossfade: Some(crossfade(3)), ..ConfigPatch::default() },
        ConfigPatch {
            theme_name: Some(ThemeName::from_static("dark")),
            crossfade: Some(crossfade(3)),
            ..ConfigPatch::default()
        }
    )]
    #[case::an_absent_field_keeps_the_earlier_one(
        ConfigPatch { device: Some(speakers()), ..ConfigPatch::default() },
        ConfigPatch::default(),
        ConfigPatch { device: Some(speakers()), ..ConfigPatch::default() }
    )]
    #[case::an_empty_account_list_is_kept(
        ConfigPatch { accounts: Some(Vec::new()), ..ConfigPatch::default() },
        ConfigPatch::default(),
        ConfigPatch { accounts: Some(Vec::new()), ..ConfigPatch::default() }
    )]
    #[case::a_later_account_list_wins(
        ConfigPatch { accounts: Some(Vec::new()), ..ConfigPatch::default() },
        ConfigPatch { accounts: Some(vec![account()]), ..ConfigPatch::default() },
        ConfigPatch { accounts: Some(vec![account()]), ..ConfigPatch::default() }
    )]
    fn config_patch_then_lets_each_present_later_field_win(
        #[case] earlier_patch: ConfigPatch,
        #[case] later: ConfigPatch,
        #[case] expected: ConfigPatch,
    ) {
        assert_eq!(earlier_patch.then(later), expected);
    }

    #[rstest]
    #[case::none_keeps_the_later_cmd(Cmd::none(), stop(), stop())]
    #[case::none_keeps_the_earlier_cmd(stop(), Cmd::none(), stop())]
    #[case::effects_in_order(
        Cmd::effect(pause()),
        stop(),
        Cmd::from_iter([pause(), Effect::Audio(AudioCmd::Stop)])
    )]
    fn then_appends_effects_in_order(
        #[case] first: Cmd,
        #[case] second: Cmd,
        #[case] expected: Cmd,
    ) {
        assert_eq!(first.then(second), expected);
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
    fn a_cmd_keeps_its_effects_in_order() {
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
        let effects: Vec<Effect> = cmd.into_parts().0;
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
                Effect::Audio(AudioCmd::Stop)
            ]
        ));
    }
}
