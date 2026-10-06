#![forbid(unsafe_code)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use kernel::{
    cmd::{Cmd, Cmds, MacosCmd},
    domain::{percent::Percent, track::Track},
    message::{MacosError, MacosEvent, OsStatus, PlaybackRequest},
    update::machine::{Driver, LoopEffect, Machine, Unhandled, each_handled},
};
use objc2::rc::{Retained, autoreleasepool};
use objc2_media_player::MPMediaItemArtwork;

use crate::{
    clock::NowPlayingClock,
    core_audio::{
        self,
        HardwareWatch,
        default_output_device,
        read_volume,
        write_volume,
    },
    cover::{Cover, CoverMessage, artwork},
    effect::MacosEffect,
    hardware::{Hardware, HardwareMessage, HardwarePoll},
    job::MacosLoopCmd,
    message::MacosMessage,
    now_playing::{NowPlaying, publish},
    remote_input::RemoteInput,
};

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

    fn take_cmds(&mut self, cmds: Cmds<MacosCmd>) -> Result<MacosLoopCmd, Unhandled> {
        let Cmds { cmds, at } = cmds;
        let last_volume = cmds.iter().rev().find_map(volume_of);
        let publish = cmds
            .iter()
            .any(|cmd| volume_of(cmd).is_none())
            .then_some(MacosEffect::Publish);
        let moved = each_handled(cmds, |cmd| self.move_now_playing(cmd, at))?;
        let tail = publish
            .into_iter()
            .chain(last_volume.map(MacosEffect::SetVolume))
            .map(LoopEffect::Execute)
            .collect();
        Ok(moved.then(tail))
    }

    fn move_now_playing(
        &mut self,
        cmd: MacosCmd,
        at: Instant,
    ) -> Result<MacosLoopCmd, Unhandled> {
        match cmd {
            MacosCmd::NowPlaying(now_playing) => {
                let track = now_playing
                    .as_deref()
                    .map(|track| track.path().to_path_buf());
                let moved = if self.cover.shows(track.as_deref()) {
                    Cmd::none()
                } else {
                    self.cover.transition(CoverMessage::TrackShown(track))?
                };
                self.now_playing = now_playing;
                self.clock = self.clock.seek(Duration::ZERO, at);
                Ok(moved)
            }
            MacosCmd::SetPlayback(playback) => {
                self.clock = self.clock.change_playback(playback, at);
                Ok(Cmd::none())
            }
            MacosCmd::SetPosition(position) => {
                self.clock = self.clock.seek(position, at);
                Ok(Cmd::none())
            }
            MacosCmd::SetVolume(_) => Ok(Cmd::none()),
        }
    }

    fn act(&mut self, effect: MacosEffect) -> Option<MacosMessage> {
        match effect {
            MacosEffect::Watch => Some(match HardwareWatch::new(self.heard.clone()) {
                Ok(watch) => {
                    self.watch = Some(watch);
                    MacosMessage::Watched
                }
                Err(error) => failed(MacosError::Listen, error),
            }),
            MacosEffect::Poll => self.poll(),
            MacosEffect::Rebind(device) => self
                .watch
                .as_mut()
                .and_then(|watch| watch.rebind_to(device).err())
                .map(|error| failed(MacosError::Rebind, error)),
            MacosEffect::SetVolume(volume) => {
                Some(match write_volume(default_output_device(), volume) {
                    Ok(()) => {
                        MacosMessage::Hardware(HardwareMessage::VolumeSet(volume))
                    }
                    Err(error) => failed(MacosError::SetVolume, error),
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
        }
    }

    fn poll(&self) -> Option<MacosMessage> {
        let watch = self.watch.as_ref()?;
        let current = default_output_device();
        Some(MacosMessage::Hardware(HardwareMessage::Polled(
            HardwarePoll {
                tracked: watch.tracked_device(),
                current,
                volume: read_volume(current),
            },
        )))
    }
}

impl Machine for MacosDriver {
    type Message = MacosMessage;
    type Effect = MacosLoopCmd;

    fn transition(&mut self, message: MacosMessage) -> Result<Self::Effect, Unhandled> {
        match message {
            MacosMessage::Started => Ok([MacosEffect::Publish, MacosEffect::Watch]
                .into_iter()
                .map(LoopEffect::Execute)
                .collect()),
            MacosMessage::Cmds(cmds) => self.take_cmds(cmds),
            MacosMessage::HardwareChanged | MacosMessage::Watched => {
                Ok(Cmd::effect(LoopEffect::Execute(MacosEffect::Poll)))
            }
            MacosMessage::Hardware(message) => Ok(self
                .hardware
                .transition(message)?
                .map_effect(LoopEffect::Execute)),
            MacosMessage::Error(error) => Ok(Cmd::message(MacosEvent::Error(error))),
            MacosMessage::CoverRead(bytes) => {
                self.cover.transition(CoverMessage::Read(bytes))
            }
            MacosMessage::Remote(input) => {
                Ok(remote(input).map_effect(LoopEffect::Execute))
            }
        }
    }
}

impl Driver for MacosDriver {
    type Effect = MacosEffect;

    fn execute(&mut self, effect: MacosEffect) -> Option<MacosMessage> {
        if let Some(watch) = &self.watch {
            watch.resend();
        }
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
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };

    use crossbeam_channel::bounded;
    use kernel::{
        cmd::{Cmd, Cmds, MacosCmd, Playback},
        domain::{
            bounded::Bounded,
            direction::Direction,
            percent::Percent,
            revision::Revision,
            track::Track,
            transport::SEEK_MEDIUM,
        },
        message::{MacosError, MacosEvent, OsStatus, PlaybackRequest},
        update::machine::{Driver, LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        clock::NowPlayingClock,
        cover::Cover,
        driver::MacosDriver,
        effect::MacosEffect,
        hardware::{Hardware, HardwareMessage, HardwarePoll},
        job::{MacosJob, MacosLoopCmd},
        message::{CoverBytes, MacosMessage},
        remote_input::RemoteInput,
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

    fn executed(loop_cmd: MacosLoopCmd) -> (Vec<MacosEffect>, Vec<MacosEvent>) {
        let (effects, events) = loop_cmd.into_parts();
        let executed = effects
            .into_iter()
            .map(|effect| match effect {
                LoopEffect::Execute(effect) => effect,
                placed @ (LoopEffect::Run(_)
                | LoopEffect::After { .. }
                | LoopEffect::Watch { .. }
                | LoopEffect::Unwatch(_)) => panic!("not executed: {placed:?}"),
            })
            .collect();
        (executed, events)
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
        MacosMessage::Error(MacosError::Listen(refused())),
        reported(MacosError::Listen(refused()))
    )]
    #[case::a_failed_rebind_is_reported(
        MacosMessage::Error(MacosError::Rebind(refused())),
        reported(MacosError::Rebind(refused()))
    )]
    #[case::a_failed_volume_write_is_reported(
        MacosMessage::Error(MacosError::SetVolume(refused())),
        reported(MacosError::SetVolume(refused()))
    )]
    #[case::a_press_is_a_media_key(
        MacosMessage::Remote(RemoteInput::Press(PlaybackRequest::SeekBy { direction: Direction::Next, by: SEEK_MEDIUM })),
        Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekBy { direction: Direction::Next, by: SEEK_MEDIUM }))
    )]
    #[case::a_hold_start_is_a_media_key(
        MacosMessage::Remote(RemoteInput::HoldBegan(PlaybackRequest::SeekBy { direction: Direction::Previous, by: SEEK_MEDIUM })),
        Cmd::message(MacosEvent::MediaKey(PlaybackRequest::SeekBy { direction: Direction::Previous, by: SEEK_MEDIUM }))
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
    #[case::a_volume_with_a_now_playing_publishes_once_and_sets_once(
        cmds(vec![volume(10), MacosCmd::NowPlaying(None), volume(20)]),
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
        assert_eq!(
            driver.transition(message).map(executed),
            Ok(cmd.into_parts())
        );
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
            driver.transition(message).map(executed),
            Ok((vec![MacosEffect::Publish], vec![]))
        );
        assert_eq!(driver.clock.playback(), playback);
    }

    type Placed = (Vec<MacosEffect>, Vec<MacosJob>, Vec<MacosEvent>);

    fn placed(loop_cmd: MacosLoopCmd) -> Placed {
        let (effects, events) = loop_cmd.into_parts();
        let (executed, jobs) = effects.into_iter().fold(
            (Vec::new(), Vec::new()),
            |(executed, jobs), effect| match effect {
                LoopEffect::Execute(effect) => {
                    ([executed, vec![effect]].concat(), jobs)
                }
                LoopEffect::Run(job) => (executed, [jobs, vec![job]].concat()),
                other @ (LoopEffect::After { .. }
                | LoopEffect::Watch { .. }
                | LoopEffect::Unwatch(_)) => panic!("not placed: {other:?}"),
            },
        );
        (executed, jobs, events)
    }

    fn polled(devices: (u32, u32), volume: Option<Percent>) -> MacosMessage {
        let (tracked, current) = devices;
        MacosMessage::Hardware(HardwareMessage::Polled(HardwarePoll {
            tracked,
            current,
            volume,
        }))
    }

    fn cover_read(bytes: &[u8]) -> MacosMessage {
        MacosMessage::CoverRead(CoverBytes {
            revision: Revision::default(),
            bytes: Ok(bytes.to_vec()),
        })
    }

    #[rstest]
    #[case::a_first_poll_reports_the_volume(
        vec![polled((1, 1), Some(Percent::clamped(30)))],
        (vec![], vec![], vec![MacosEvent::Volume(Percent::clamped(30))])
    )]
    #[case::a_poll_on_another_device_rebinds(
        vec![polled((1, 2), None)],
        (vec![MacosEffect::Rebind(2)], vec![], vec![])
    )]
    #[case::a_volume_set_is_quiet(
        vec![MacosMessage::Hardware(HardwareMessage::VolumeSet(Percent::clamped(40)))],
        (vec![], vec![], vec![])
    )]
    #[case::the_echo_of_our_volume_is_quiet(
        vec![MacosMessage::Hardware(HardwareMessage::VolumeSet(Percent::clamped(40))), polled((1, 1), Some(Percent::clamped(40)))],
        (vec![], vec![], vec![])
    )]
    #[case::cover_bytes_show_the_artwork(
        vec![cover_read(&[1, 2])],
        (vec![MacosEffect::ShowArtwork(vec![1, 2]), MacosEffect::Publish], vec![], vec![])
    )]
    #[case::a_new_track_clears_reads_then_publishes(
        vec![cmds(vec![MacosCmd::NowPlaying(Some(Arc::new(Track::listed(Path::new("a.flac")))))])],
        (
            vec![MacosEffect::ClearArtwork, MacosEffect::Publish],
            vec![MacosJob::ReadCover { track: PathBuf::from("a.flac"), revision: Revision::default().next() }],
            vec![]
        )
    )]
    fn the_macos_driver_routes_to_its_parts(
        #[case] messages: Vec<MacosMessage>,
        #[case] last: Placed,
    ) {
        let (heard, _heard_receiver) = bounded(1);
        let mut driver = MacosDriver::new(heard);
        let answers: Vec<_> = messages
            .into_iter()
            .map(|message| driver.transition(message).map(placed))
            .collect();
        assert_eq!(answers.last(), Some(&Ok(last)));
    }

    #[test]
    fn a_refused_cover_leaves_the_driver_and_the_rest_of_the_batch_intact() {
        let (heard, _heard_receiver) = bounded(1);
        let mut macos_driver = MacosDriver::new(heard);
        let shown = Arc::new(Track::listed(Path::new("a.flac")));
        let first = cmds(vec![MacosCmd::NowPlaying(Some(Arc::clone(&shown)))]);
        assert!(macos_driver.transition(first).is_ok());
        let before = macos_driver.cover.clone();
        let again = cmds(vec![
            MacosCmd::NowPlaying(Some(Arc::clone(&shown))),
            volume(20),
        ]);
        assert_eq!(
            macos_driver.transition(again).map(executed),
            Ok((
                vec![
                    MacosEffect::Publish,
                    MacosEffect::SetVolume(Percent::clamped(20)),
                ],
                vec![]
            ))
        );
        assert_eq!(macos_driver.cover, before);
        assert_eq!(macos_driver.now_playing, Some(shown));
    }

    #[test]
    fn empty_cover_bytes_are_refused() {
        let (heard, _heard_receiver) = bounded(1);
        let mut macos_driver = MacosDriver::new(heard);
        let before = macos_driver.cover.clone();
        assert_eq!(
            macos_driver.transition(cover_read(&[])).map(placed),
            Err(Unhandled)
        );
        assert_eq!(macos_driver.cover, before);
        assert!(macos_driver.artwork.is_none());
    }

    #[derive(Debug, PartialEq)]
    struct Held {
        cover: Cover,
        track: Option<Arc<Track>>,
        clock: NowPlayingClock,
        hardware: Hardware,
    }

    fn held(macos_driver: &MacosDriver) -> Held {
        Held {
            cover: macos_driver.cover.clone(),
            track: macos_driver.now_playing.clone(),
            clock: macos_driver.clock,
            hardware: macos_driver.hardware,
        }
    }

    #[rstest]
    #[case::an_empty_batch(vec![], cmds(vec![]))]
    #[case::a_poll_that_changes_nothing(
        vec![polled((1, 1), None)],
        polled((1, 1), None)
    )]
    #[case::cover_bytes_of_nothing(vec![], cover_read(&[]))]
    fn the_macos_driver_refuses_what_changes_nothing(
        #[case] earlier_macos_messages: Vec<MacosMessage>,
        #[case] refused_macos_message: MacosMessage,
    ) {
        let (heard, _heard_receiver) = bounded(1);
        let mut macos_driver = MacosDriver::new(heard);
        for macos_message in earlier_macos_messages {
            assert!(macos_driver.transition(macos_message).is_ok());
        }
        let before = held(&macos_driver);
        assert_eq!(
            macos_driver.transition(refused_macos_message).map(placed),
            Err(Unhandled)
        );
        assert_eq!(held(&macos_driver), before);
    }

    #[test]
    fn a_stale_cover_read_is_refused_and_leaves_the_cover_alone() {
        let (heard, _heard_receiver) = bounded(1);
        let mut macos_driver = MacosDriver::new(heard);
        let before = macos_driver.cover.clone();
        let macos_message = MacosMessage::CoverRead(CoverBytes {
            revision: Revision::default().next(),
            bytes: Ok(vec![1, 2]),
        });
        assert_eq!(
            macos_driver.transition(macos_message).map(placed),
            Err(Unhandled)
        );
        assert_eq!(macos_driver.cover, before);
    }

    #[rstest]
    #[case::a_rebind_without_a_watch(MacosEffect::Rebind(2))]
    #[case::a_poll_without_a_watch(MacosEffect::Poll)]
    #[case::clearing_the_artwork(MacosEffect::ClearArtwork)]
    #[case::showing_no_artwork(MacosEffect::ShowArtwork(vec![]))]
    fn the_driver_answers_nothing_and_holds_no_artwork(
        #[case] macos_effect: MacosEffect,
    ) {
        let (heard, _heard_receiver) = bounded(1);
        let mut driver = MacosDriver::new(heard);
        assert!(driver.execute(macos_effect).is_none());
        assert!(driver.artwork.is_none());
    }
}
