#![forbid(unsafe_code)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use kernel::{
    Cmd,
    Cmds,
    MacosCmd,
    MacosError,
    MacosEvent,
    Percent,
    PlaybackRequest,
    Track,
    message::OsStatus,
    update::{Driver, Machine, Unhandled},
};
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_audio::AudioObjectID;
use objc2_media_player::MPMediaItemArtwork;

use crate::{
    clock::NowPlayingClock,
    controls::RemoteInput,
    core_audio::{
        self,
        HardwareWatch,
        default_output_device,
        read_volume,
        write_volume,
    },
    cover::{Cover, CoverBytes, CoverMessage, MacosJob, artwork},
    hardware::{Hardware, HardwareMessage, HardwarePoll},
    now_playing::{NowPlaying, publish},
};

#[derive(Debug)]
pub enum MacosMessage {
    Started,
    Cmds(Cmds<MacosCmd>),
    HardwareChanged,
    Watched,
    Polled(HardwarePoll),
    Rebound(AudioObjectID),
    VolumeSet(Percent),
    Error(MacosError),
    CoverRead(CoverBytes),
    Remote(RemoteInput),
}

impl From<Cmds<MacosCmd>> for MacosMessage {
    fn from(cmds: Cmds<MacosCmd>) -> Self {
        MacosMessage::Cmds(cmds)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosEffect {
    Watch,
    Poll,
    Rebind(AudioObjectID),
    SetVolume(Percent),
    Publish,
    ClearArtwork,
    ShowArtwork(Vec<u8>),
    Run(MacosJob),
}

#[derive(Debug)]
pub struct MacosDriver {
    heard: Sender<MacosMessage>,
    now_playing: Option<Arc<Track>>,
    clock: NowPlayingClock,
    cover: Cover,
    hardware: Hardware,
    artwork: Option<Retained<MPMediaItemArtwork>>,
    watch: Option<HardwareWatch>,
}

impl MacosDriver {
    #[must_use]
    pub fn new(heard: Sender<MacosMessage>) -> Self {
        Self {
            heard,
            now_playing: None,
            clock: NowPlayingClock::default(),
            cover: Cover::default(),
            hardware: Hardware::default(),
            artwork: None,
            watch: None,
        }
    }

    fn take_cmds(
        &mut self,
        cmds: Cmds<MacosCmd>,
    ) -> Result<Cmd<MacosEffect, MacosEvent>, Unhandled> {
        let Cmds { cmds, at } = cmds;
        let last_volume = cmds.iter().rev().find_map(volume_of);
        let now_playing: Vec<MacosCmd> = cmds
            .into_iter()
            .filter(|cmd| volume_of(cmd).is_none())
            .collect();
        let publish = (!now_playing.is_empty()).then_some(MacosEffect::Publish);
        let moved = now_playing.into_iter().try_fold(
            Cmd::none(),
            |moved: Cmd<MacosEffect, MacosEvent>, cmd| {
                Ok::<_, Unhandled>(moved.then(self.move_now_playing(cmd, at)?))
            },
        )?;
        let tail = publish
            .into_iter()
            .chain(last_volume.map(MacosEffect::SetVolume))
            .collect();
        Ok(moved.then(tail))
    }

    fn move_now_playing(
        &mut self,
        cmd: MacosCmd,
        at: Instant,
    ) -> Result<Cmd<MacosEffect, MacosEvent>, Unhandled> {
        match cmd {
            MacosCmd::NowPlaying(now_playing) => {
                let track = now_playing
                    .as_deref()
                    .map(|track| track.path().to_path_buf());
                self.now_playing = now_playing;
                self.clock = self.clock.seek(Duration::ZERO, at);
                self.cover.transition(CoverMessage::TrackShown(track))
            }
            MacosCmd::SetPlayback(playback) => {
                self.clock = self.clock.change_playback(playback, at);
                Ok(Cmd::none())
            }
            MacosCmd::SetPosition(position) => {
                self.clock = self.clock.seek(position, at);
                Ok(Cmd::none())
            }
            MacosCmd::SetVolume(volume) => {
                Ok(Cmd::effect(MacosEffect::SetVolume(volume)))
            }
        }
    }

    fn act(&mut self, effect: MacosEffect) -> Option<MacosMessage> {
        match effect {
            MacosEffect::Watch => Some(match HardwareWatch::new(self.heard.clone()) {
                Ok(watch) => {
                    self.watch = Some(watch);
                    MacosMessage::Watched
                }
                Err(error) => failed(MacosError::HardwareWatch, error),
            }),
            MacosEffect::Poll => self.poll(),
            MacosEffect::Rebind(device) => self.watch.as_mut().map(|watch| match watch
                .rebind_to(device)
            {
                Ok(()) => MacosMessage::Rebound(device),
                Err(error) => failed(MacosError::Rebind, error),
            }),
            MacosEffect::SetVolume(volume) => {
                Some(match write_volume(default_output_device(), volume) {
                    Ok(()) => MacosMessage::VolumeSet(volume),
                    Err(error) => failed(MacosError::Volume, error),
                })
            }
            MacosEffect::Publish => {
                let shown = NowPlaying {
                    track: self.now_playing.as_deref(),
                    clock: self.clock,
                    artwork: self.artwork.as_deref(),
                };
                publish(shown, Instant::now());
                None
            }
            MacosEffect::ClearArtwork => {
                self.artwork = None;
                None
            }
            MacosEffect::ShowArtwork(bytes) => {
                self.artwork = artwork(&bytes);
                None
            }
            MacosEffect::Run(_job) => None,
        }
    }

    fn poll(&self) -> Option<MacosMessage> {
        let watch = self.watch.as_ref()?;
        let current_device = default_output_device();
        Some(MacosMessage::Polled(HardwarePoll {
            tracked_device: watch.tracked_device(),
            current_device,
            volume: read_volume(current_device),
        }))
    }
}

impl Machine for MacosDriver {
    type Message = MacosMessage;
    type Effect = Cmd<MacosEffect, MacosEvent>;

    fn transition(&mut self, message: MacosMessage) -> Result<Self::Effect, Unhandled> {
        match message {
            MacosMessage::Started => Ok([MacosEffect::Publish, MacosEffect::Watch]
                .into_iter()
                .collect()),
            MacosMessage::Cmds(cmds) => self.take_cmds(cmds),
            MacosMessage::HardwareChanged | MacosMessage::Watched => {
                Ok(Cmd::effect(MacosEffect::Poll))
            }
            MacosMessage::Polled(poll) => {
                self.hardware.transition(HardwareMessage::Polled(poll))
            }
            MacosMessage::Rebound(_device) => Ok(Cmd::none()),
            MacosMessage::VolumeSet(volume) => {
                self.hardware.transition(HardwareMessage::VolumeSet(volume))
            }
            MacosMessage::Error(error) => Ok(Cmd::message(MacosEvent::Error(error))),
            MacosMessage::CoverRead(bytes) => {
                self.cover.transition(CoverMessage::Read(bytes))
            }
            MacosMessage::Remote(input) => Ok(remote(input)),
        }
    }
}

impl Driver for MacosDriver {
    type Effect = MacosEffect;

    fn execute(&mut self, effect: MacosEffect) -> Option<MacosMessage> {
        autoreleasepool(|_| self.act(effect))
    }
}

fn volume_of(cmd: &MacosCmd) -> Option<Percent> {
    match cmd {
        MacosCmd::SetVolume(volume) => Some(*volume),
        MacosCmd::NowPlaying(_)
        | MacosCmd::SetPlayback(_)
        | MacosCmd::SetPosition(_) => None,
    }
}

fn remote(input: RemoteInput) -> Cmd<MacosEffect, MacosEvent> {
    match input {
        RemoteInput::Press(request) | RemoteInput::HoldBegan(request) => {
            Cmd::message(MacosEvent::MediaKey(request))
        }
        RemoteInput::HoldEnded => Cmd::none(),
        RemoteInput::Scrub(at) => {
            Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekTo(at)))
        }
    }
}

fn failed(cause: fn(OsStatus) -> MacosError, error: core_audio::Error) -> MacosMessage {
    MacosMessage::Error(cause(error.status()))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossbeam_channel::bounded;
    use kernel::{
        Bounded,
        Cmd,
        Cmds,
        MacosCmd,
        MacosError,
        MacosEvent,
        Percent,
        Playback,
        PlaybackRequest,
        message::OsStatus,
        update::Machine,
    };
    use rstest::rstest;

    use crate::{
        controls::RemoteInput,
        driver::{MacosDriver, MacosEffect, MacosMessage},
    };

    fn volume(level: u8) -> MacosCmd {
        MacosCmd::SetVolume(Percent::clamped(level))
    }

    fn cmds(cmds: Vec<MacosCmd>) -> MacosMessage {
        MacosMessage::Cmds(Cmds {
            cmds,
            at: Instant::now(),
        })
    }

    fn refused() -> OsStatus {
        OsStatus(-50)
    }

    fn reported(error: MacosError) -> Cmd<MacosEffect, MacosEvent> {
        Cmd::message(MacosEvent::Error(error))
    }

    #[rstest]
    #[case::started_publishes_then_watches(
        MacosMessage::Started,
        [MacosEffect::Publish, MacosEffect::Watch].into_iter().collect()
    )]
    #[case::a_hardware_change_polls(
        MacosMessage::HardwareChanged,
        Cmd::effect(MacosEffect::Poll)
    )]
    #[case::a_watch_in_place_polls(
        MacosMessage::Watched,
        Cmd::effect(MacosEffect::Poll)
    )]
    #[case::a_failed_watch_is_reported(
        MacosMessage::Error(MacosError::HardwareWatch(refused())),
        reported(MacosError::HardwareWatch(refused()))
    )]
    #[case::a_failed_rebind_is_reported(
        MacosMessage::Error(MacosError::Rebind(refused())),
        reported(MacosError::Rebind(refused()))
    )]
    #[case::a_failed_volume_write_is_reported(
        MacosMessage::Error(MacosError::Volume(refused())),
        reported(MacosError::Volume(refused()))
    )]
    #[case::a_rebind_is_quiet(MacosMessage::Rebound(2), Cmd::none())]
    #[case::a_press_is_a_media_key(
        MacosMessage::Remote(RemoteInput::Press(PlaybackRequest::SeekForward)),
        Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekForward))
    )]
    #[case::a_hold_start_is_a_media_key(
        MacosMessage::Remote(RemoteInput::HoldBegan(PlaybackRequest::SeekBack)),
        Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekBack))
    )]
    #[case::a_hold_end_is_quiet(
        MacosMessage::Remote(RemoteInput::HoldEnded),
        Cmd::none()
    )]
    #[case::a_scrub_seeks(
        MacosMessage::Remote(RemoteInput::Scrub(Duration::from_secs(3))),
        Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekTo(
            Duration::from_secs(3)
        )))
    )]
    #[case::three_volumes_keep_the_last(
        cmds(vec![volume(10), volume(20), volume(30)]),
        Cmd::effect(MacosEffect::SetVolume(Percent::clamped(30)))
    )]
    #[case::volumes_among_other_commands_go_last(
        cmds(vec![
            MacosCmd::SetPlayback(Playback::Playing),
            volume(10),
            MacosCmd::SetPlayback(Playback::Paused),
            volume(20),
        ]),
        [
            MacosEffect::Publish,
            MacosEffect::SetVolume(Percent::clamped(20)),
        ].into_iter().collect()
    )]
    #[case::no_volume_at_all(
        cmds(vec![MacosCmd::SetPlayback(Playback::Playing)]),
        Cmd::effect(MacosEffect::Publish)
    )]
    #[case::nothing_playing_yet_keeps_the_artwork(
        cmds(vec![MacosCmd::NowPlaying(None)]),
        Cmd::effect(MacosEffect::Publish)
    )]
    fn the_macos_driver_answers_each_message(
        #[case] message: MacosMessage,
        #[case] cmd: Cmd<MacosEffect, MacosEvent>,
    ) {
        let (heard, _heard_receiver) = bounded(1);
        let mut driver = MacosDriver::new(heard);
        assert_eq!(driver.transition(message), Ok(cmd));
    }

    #[rstest]
    #[case::playing(Playback::Playing)]
    #[case::paused(Playback::Paused)]
    fn the_now_playing_clock_follows_the_last_playback_command(
        #[case] playback: Playback,
    ) {
        let (heard, _heard_receiver) = bounded(1);
        let mut driver = MacosDriver::new(heard);
        let message = cmds(vec![
            MacosCmd::SetPlayback(Playback::Playing),
            MacosCmd::SetPlayback(playback),
        ]);
        assert_eq!(
            driver.transition(message),
            Ok(Cmd::effect(MacosEffect::Publish))
        );
        assert_eq!(driver.clock.playback(), playback);
    }
}
